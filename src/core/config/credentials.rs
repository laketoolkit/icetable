//! Credential management
//!
//! Provides a unified interface for handling credentials from different sources
//! (environment variables, files, IAM roles, etc.) with security best practices.

use serde::{Deserialize, Deserializer, Serialize, Serializer, de as serde_de};
use std::fmt;
use std::path::PathBuf;

use crate::error::{Error, Result};

/// Source of credentials with security considerations
#[derive(Debug, Clone, PartialEq)]
pub enum CredentialSource {
    /// Inline credential (stored in plain text - NOT RECOMMENDED for production)
    Inline(String),
    /// Read from environment variable
    EnvVar(String),
    /// Read from file (e.g., mounted Kubernetes secret)
    File(PathBuf),
    /// Use IAM role (AWS, GCP, Azure) - auto-detected from environment
    IamRole,
    /// Use OAuth2 token - auto-refreshed by cloud SDK
    OAuth2,
}

// Serialization helpers
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum CredentialType {
    Inline,
    EnvVar,
    File,
    IamRole,
    OAuth2,
}

#[derive(Serialize, Deserialize)]
struct CredentialWrapper {
    #[serde(rename = "type")]
    typ: CredentialType,
    value: Option<String>,
}

impl Serialize for CredentialSource {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            CredentialSource::Inline(s) => {
                // For backward compatibility: plain strings serialize as simple YAML strings
                serializer.serialize_str(s)
            }
            CredentialSource::EnvVar(var) => {
                let wrapper = CredentialWrapper {
                    typ: CredentialType::EnvVar,
                    value: Some(var.clone()),
                };
                wrapper.serialize(serializer)
            }
            CredentialSource::File(path) => {
                let wrapper = CredentialWrapper {
                    typ: CredentialType::File,
                    value: Some(path.to_string_lossy().to_string()),
                };
                wrapper.serialize(serializer)
            }
            CredentialSource::IamRole => {
                let wrapper = CredentialWrapper {
                    typ: CredentialType::IamRole,
                    value: None,
                };
                wrapper.serialize(serializer)
            }
            CredentialSource::OAuth2 => {
                let wrapper = CredentialWrapper {
                    typ: CredentialType::OAuth2,
                    value: None,
                };
                wrapper.serialize(serializer)
            }
        }
    }
}

impl<'de> Deserialize<'de> for CredentialSource {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct CredentialVisitor;

        impl<'de> serde_de::Visitor<'de> for CredentialVisitor {
            type Value = CredentialSource;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("string or credential object")
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
            where
                E: serde_de::Error,
            {
                // Plain string → Inline credential (backward compatibility)
                Ok(CredentialSource::Inline(value.to_string()))
            }

            fn visit_string<E>(self, value: String) -> std::result::Result<Self::Value, E>
            where
                E: serde_de::Error,
            {
                Ok(CredentialSource::Inline(value))
            }

            fn visit_map<A>(self, map: A) -> std::result::Result<Self::Value, A::Error>
            where
                A: serde_de::MapAccess<'de>,
            {
                let wrapper: CredentialWrapper =
                    Deserialize::deserialize(serde_de::value::MapAccessDeserializer::new(map))?;

                match wrapper.typ {
                    CredentialType::Inline => {
                        let value = wrapper
                            .value
                            .ok_or_else(|| serde_de::Error::missing_field("value"))?;
                        Ok(CredentialSource::Inline(value))
                    }
                    CredentialType::EnvVar => {
                        let value = wrapper
                            .value
                            .ok_or_else(|| serde_de::Error::missing_field("value"))?;
                        Ok(CredentialSource::EnvVar(value))
                    }
                    CredentialType::File => {
                        let value = wrapper
                            .value
                            .ok_or_else(|| serde_de::Error::missing_field("value"))?;
                        Ok(CredentialSource::File(PathBuf::from(value)))
                    }
                    CredentialType::IamRole => Ok(CredentialSource::IamRole),
                    CredentialType::OAuth2 => Ok(CredentialSource::OAuth2),
                }
            }
        }

        deserializer.deserialize_any(CredentialVisitor)
    }
}

impl CredentialSource {
    /// Resolve credential to a string token/secret
    pub fn resolve(&self) -> Result<Option<String>> {
        match self {
            CredentialSource::Inline(s) => Ok(Some(s.clone())),
            CredentialSource::EnvVar(var) => {
                std::env::var(var)
                    .map(Some)
                    .map_err(|_| Error::Configuration {
                        message: format!("Environment variable '{}' not found", var),
                    })
            }
            CredentialSource::File(path) => std::fs::read_to_string(path)
                .map(|s| Some(s.trim().to_string()))
                .map_err(|e| Error::Configuration {
                    message: format!("Failed to read credential file '{}': {}", path.display(), e),
                }),
            CredentialSource::IamRole => {
                // Auto-detected IAM role - no explicit credential needed
                // Cloud SDKs will use instance metadata service
                Ok(None)
            }
            CredentialSource::OAuth2 => {
                // OAuth2 tokens are obtained automatically by cloud SDKs
                Ok(None)
            }
        }
    }

    /// Get a user-friendly description of the credential source
    pub fn describe(&self) -> String {
        match self {
            CredentialSource::Inline(_) => "inline (plain text)".to_string(),
            CredentialSource::EnvVar(var) => format!("env:{}", var),
            CredentialSource::File(path) => format!("file:{}", path.display()),
            CredentialSource::IamRole => "iam-role".to_string(),
            CredentialSource::OAuth2 => "oauth2".to_string(),
        }
    }

    /// Create credential source from CLI arguments
    pub fn from_cli_options(
        inline: Option<String>,
        env_var: Option<String>,
        file: Option<PathBuf>,
        iam_role: bool,
        oauth2: bool,
    ) -> Option<Self> {
        match (inline, env_var, file, iam_role, oauth2) {
            (Some(token), _, _, _, _) => Some(CredentialSource::Inline(token)),
            (_, Some(var), _, _, _) => Some(CredentialSource::EnvVar(var)),
            (_, _, Some(path), _, _) => Some(CredentialSource::File(path)),
            (_, _, _, true, _) => Some(CredentialSource::IamRole),
            (_, _, _, _, true) => Some(CredentialSource::OAuth2),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::NamedTempFile;

    // Helper to safely set env var for tests (runs serially to avoid races)
    fn with_env_var<F, R>(key: &str, value: &str, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        // SAFETY: This is safe because tests using this helper run serially
        unsafe {
            std::env::set_var(key, value);
        }
        let result = f();
        unsafe {
            std::env::remove_var(key);
        }
        result
    }

    #[test]
    #[serial]
    fn test_inline_credential() {
        with_env_var("ICETABLE_SUPPRESS_CREDENTIAL_WARNINGS", "1", || {
            let cred = CredentialSource::Inline("token123".to_string());
            assert_eq!(cred.resolve().unwrap(), Some("token123".to_string()));
            assert_eq!(cred.describe(), "inline (plain text)");
        });
    }

    #[test]
    #[serial]
    fn test_env_var_credential() {
        with_env_var("TEST_CRED_TOKEN", "secret123", || {
            let cred = CredentialSource::EnvVar("TEST_CRED_TOKEN".to_string());
            assert_eq!(cred.resolve().unwrap(), Some("secret123".to_string()));
            assert_eq!(cred.describe(), "env:TEST_CRED_TOKEN");
        });
    }

    #[test]
    fn test_file_credential() {
        let temp_file = NamedTempFile::new().unwrap();
        std::fs::write(temp_file.path(), "file-token\n").unwrap();

        let cred = CredentialSource::File(temp_file.path().to_path_buf());
        assert_eq!(cred.resolve().unwrap(), Some("file-token".to_string()));
        assert!(cred.describe().contains("file:"));
    }

    #[test]
    fn test_iam_role_credential() {
        let cred = CredentialSource::IamRole;
        assert_eq!(cred.resolve().unwrap(), None);
        assert_eq!(cred.describe(), "iam-role");
    }

    #[test]
    fn test_from_cli_options() {
        let cred =
            CredentialSource::from_cli_options(Some("token".to_string()), None, None, false, false);
        assert!(matches!(cred, Some(CredentialSource::Inline(_))));

        let cred =
            CredentialSource::from_cli_options(None, Some("VAR".to_string()), None, false, false);
        assert!(matches!(cred, Some(CredentialSource::EnvVar(_))));

        let cred = CredentialSource::from_cli_options(
            None,
            None,
            Some(PathBuf::from("/tmp/token")),
            false,
            false,
        );
        assert!(matches!(cred, Some(CredentialSource::File(_))));

        let cred = CredentialSource::from_cli_options(None, None, None, true, false);
        assert!(matches!(cred, Some(CredentialSource::IamRole)));

        let cred = CredentialSource::from_cli_options(None, None, None, false, true);
        assert!(matches!(cred, Some(CredentialSource::OAuth2)));

        let cred = CredentialSource::from_cli_options(None, None, None, false, false);
        assert!(cred.is_none());
    }

    #[test]
    fn test_yaml_serialization() {
        // Inline serializes as plain string
        let cred = CredentialSource::Inline("secret".to_string());
        let yaml = serde_yaml::to_string(&cred).unwrap();
        assert_eq!(yaml.trim(), "secret");

        // EnvVar serializes as object
        let cred = CredentialSource::EnvVar("MY_TOKEN".to_string());
        let yaml = serde_yaml::to_string(&cred).unwrap();
        assert!(yaml.contains("type: env-var"));
        assert!(yaml.contains("value: MY_TOKEN"));
    }
}
