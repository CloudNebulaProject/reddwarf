use reddwarf_core::ResourceKey;
use std::fmt;

/// Key encoder for storage keys
pub struct KeyEncoder;

impl KeyEncoder {
    /// Encode a resource key: {api_version}/{kind}/{namespace}/{name}
    /// For cluster-scoped: {api_version}/{kind}/{name}
    pub fn encode_resource_key(key: &ResourceKey) -> String {
        key.storage_key()
    }

    /// Encode a prefix for scanning resources of a kind in a namespace
    pub fn encode_prefix(api_version: &str, kind: &str, namespace: Option<&str>) -> String {
        if let Some(ns) = namespace {
            format!("{}/{}/{}/", api_version, kind, ns)
        } else {
            format!("{}/{}/", api_version, kind)
        }
    }

    /// Encode a namespace prefix for scanning all resources in a namespace
    pub fn encode_namespace_prefix(namespace: &str) -> String {
        // This will match any resource with this namespace
        // We'll need to filter by namespace during scan
        namespace.to_string()
    }

    /// Parse a storage key back to components
    pub fn parse_key(key: &str) -> Option<(String, String, Option<String>, String)> {
        let parts: Vec<&str> = key.split('/').collect();

        match parts.len() {
            3 => {
                // Cluster-scoped: api_version/kind/name
                Some((
                    parts[0].to_string(),
                    parts[1].to_string(),
                    None,
                    parts[2].to_string(),
                ))
            }
            4 => {
                // Namespaced: api_version/kind/namespace/name
                Some((
                    parts[0].to_string(),
                    parts[1].to_string(),
                    Some(parts[2].to_string()),
                    parts[3].to_string(),
                ))
            }
            _ => None,
        }
    }
}

/// Index key types for secondary indices
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexKey {
    /// Index by namespace: namespace/{namespace}/{api_version}/{kind}/{name}
    Namespace {
        namespace: String,
        api_version: String,
        kind: String,
        name: String,
    },
    /// Index by label: label/{key}/{value}/{api_version}/{kind}/{namespace}/{name}
    Label {
        key: String,
        value: String,
        api_version: String,
        kind: String,
        namespace: Option<String>,
        name: String,
    },
    /// Index by field: field/{field_path}/{value}/{api_version}/{kind}/{namespace}/{name}
    Field {
        field_path: String,
        value: String,
        api_version: String,
        kind: String,
        namespace: Option<String>,
        name: String,
    },
}

impl IndexKey {
    /// Encode the index key to a string
    pub fn encode(&self) -> String {
        match self {
            IndexKey::Namespace {
                namespace,
                api_version,
                kind,
                name,
            } => format!("namespace/{}/{}/{}/{}", namespace, api_version, kind, name),
            IndexKey::Label {
                key,
                value,
                api_version,
                kind,
                namespace,
                name,
            } => {
                if let Some(ns) = namespace {
                    format!("label/{}/{}/{}/{}/{}/{}", key, value, api_version, kind, ns, name)
                } else {
                    format!("label/{}/{}/{}/{}/{}", key, value, api_version, kind, name)
                }
            }
            IndexKey::Field {
                field_path,
                value,
                api_version,
                kind,
                namespace,
                name,
            } => {
                if let Some(ns) = namespace {
                    format!("field/{}/{}/{}/{}/{}/{}", field_path, value, api_version, kind, ns, name)
                } else {
                    format!("field/{}/{}/{}/{}/{}", field_path, value, api_version, kind, name)
                }
            }
        }
    }

    /// Encode a prefix for scanning
    pub fn encode_prefix_for_namespace(namespace: &str) -> String {
        format!("namespace/{}/", namespace)
    }

    /// Encode a prefix for scanning by label
    pub fn encode_prefix_for_label(key: &str, value: Option<&str>) -> String {
        if let Some(v) = value {
            format!("label/{}/{}/", key, v)
        } else {
            format!("label/{}/", key)
        }
    }

    /// Encode a prefix for scanning by field
    pub fn encode_prefix_for_field(field_path: &str, value: Option<&str>) -> String {
        if let Some(v) = value {
            format!("field/{}/{}/", field_path, v)
        } else {
            format!("field/{}/", field_path)
        }
    }
}

impl fmt::Display for IndexKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.encode())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reddwarf_core::GroupVersionKind;

    #[test]
    fn test_encode_resource_key() {
        let gvk = GroupVersionKind::from_api_version_kind("v1", "Pod");
        let key = ResourceKey::new(gvk, "default", "nginx");
        assert_eq!(KeyEncoder::encode_resource_key(&key), "v1/Pod/default/nginx");

        let gvk = GroupVersionKind::from_api_version_kind("v1", "Node");
        let key = ResourceKey::cluster_scoped(gvk, "node-1");
        assert_eq!(KeyEncoder::encode_resource_key(&key), "v1/Node/node-1");
    }

    #[test]
    fn test_encode_prefix() {
        assert_eq!(
            KeyEncoder::encode_prefix("v1", "Pod", Some("default")),
            "v1/Pod/default/"
        );
        assert_eq!(KeyEncoder::encode_prefix("v1", "Node", None), "v1/Node/");
    }

    #[test]
    fn test_parse_key() {
        let (api_version, kind, namespace, name) =
            KeyEncoder::parse_key("v1/Pod/default/nginx").unwrap();
        assert_eq!(api_version, "v1");
        assert_eq!(kind, "Pod");
        assert_eq!(namespace, Some("default".to_string()));
        assert_eq!(name, "nginx");

        let (api_version, kind, namespace, name) = KeyEncoder::parse_key("v1/Node/node-1").unwrap();
        assert_eq!(api_version, "v1");
        assert_eq!(kind, "Node");
        assert_eq!(namespace, None);
        assert_eq!(name, "node-1");
    }

    #[test]
    fn test_index_key_namespace() {
        let key = IndexKey::Namespace {
            namespace: "default".to_string(),
            api_version: "v1".to_string(),
            kind: "Pod".to_string(),
            name: "nginx".to_string(),
        };
        assert_eq!(key.encode(), "namespace/default/v1/Pod/nginx");
    }

    #[test]
    fn test_index_key_label() {
        let key = IndexKey::Label {
            key: "app".to_string(),
            value: "nginx".to_string(),
            api_version: "v1".to_string(),
            kind: "Pod".to_string(),
            namespace: Some("default".to_string()),
            name: "nginx-pod".to_string(),
        };
        assert_eq!(key.encode(), "label/app/nginx/v1/Pod/default/nginx-pod");
    }
}
