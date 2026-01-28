use serde::{Deserialize, Serialize};
use std::fmt;

/// GroupVersionKind uniquely identifies a Kubernetes resource type
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GroupVersionKind {
    /// API group (e.g., "", "apps", "batch")
    pub group: String,
    /// API version (e.g., "v1", "v1beta1")
    pub version: String,
    /// Resource kind (e.g., "Pod", "Deployment")
    pub kind: String,
}

impl GroupVersionKind {
    /// Create a new GVK
    pub fn new(group: impl Into<String>, version: impl Into<String>, kind: impl Into<String>) -> Self {
        Self {
            group: group.into(),
            version: version.into(),
            kind: kind.into(),
        }
    }

    /// Create a GVK from apiVersion and kind
    /// apiVersion format: "v1" or "group/version"
    pub fn from_api_version_kind(api_version: &str, kind: &str) -> Self {
        let (group, version) = if let Some(idx) = api_version.find('/') {
            let (g, v) = api_version.split_at(idx);
            (g.to_string(), v[1..].to_string())
        } else {
            (String::new(), api_version.to_string())
        };

        Self {
            group,
            version,
            kind: kind.to_string(),
        }
    }

    /// Get the apiVersion string (group/version or just version)
    pub fn api_version(&self) -> String {
        if self.group.is_empty() {
            self.version.clone()
        } else {
            format!("{}/{}", self.group, self.version)
        }
    }

    /// Get the full API path segment
    pub fn api_path(&self) -> String {
        if self.group.is_empty() {
            format!("api/{}", self.version)
        } else {
            format!("apis/{}/{}", self.group, self.version)
        }
    }

    /// Get the resource name (lowercase, plural)
    pub fn resource_name(&self) -> String {
        // Simple pluralization - should be enhanced for production
        let lower = self.kind.to_lowercase();
        if lower.ends_with('s') {
            format!("{}es", lower)
        } else if lower.ends_with('y') {
            format!("{}ies", &lower[..lower.len() - 1])
        } else {
            format!("{}s", lower)
        }
    }
}

impl fmt::Display for GroupVersionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.api_version(), self.kind)
    }
}

/// ResourceKey uniquely identifies a specific resource instance
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResourceKey {
    /// GroupVersionKind of the resource
    pub gvk: GroupVersionKind,
    /// Namespace (empty for cluster-scoped resources)
    pub namespace: String,
    /// Resource name
    pub name: String,
}

impl ResourceKey {
    /// Create a new ResourceKey
    pub fn new(gvk: GroupVersionKind, namespace: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            gvk,
            namespace: namespace.into(),
            name: name.into(),
        }
    }

    /// Create a cluster-scoped ResourceKey
    pub fn cluster_scoped(gvk: GroupVersionKind, name: impl Into<String>) -> Self {
        Self {
            gvk,
            namespace: String::new(),
            name: name.into(),
        }
    }

    /// Check if this is a namespaced resource
    pub fn is_namespaced(&self) -> bool {
        !self.namespace.is_empty()
    }

    /// Get the storage key encoding: {api_version}/{kind}/{namespace}/{name}
    /// For cluster-scoped: {api_version}/{kind}/{name}
    pub fn storage_key(&self) -> String {
        let api_version = self.gvk.api_version();
        if self.is_namespaced() {
            format!("{}/{}/{}/{}", api_version, self.gvk.kind, self.namespace, self.name)
        } else {
            format!("{}/{}/{}", api_version, self.gvk.kind, self.name)
        }
    }

    /// Get the API path for this resource
    pub fn api_path(&self) -> String {
        let base = self.gvk.api_path();
        let resource = self.gvk.resource_name();

        if self.is_namespaced() {
            format!("/{}/namespaces/{}/{}/{}", base, self.namespace, resource, self.name)
        } else {
            format!("/{}/{}/{}", base, resource, self.name)
        }
    }

    /// Get the API path for the collection (without name)
    pub fn collection_path(&self) -> String {
        let base = self.gvk.api_path();
        let resource = self.gvk.resource_name();

        if self.is_namespaced() {
            format!("/{}/namespaces/{}/{}", base, self.namespace, resource)
        } else {
            format!("/{}/{}", base, resource)
        }
    }
}

impl fmt::Display for ResourceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_namespaced() {
            write!(f, "{}/{}/{}", self.gvk, self.namespace, self.name)
        } else {
            write!(f, "{}/{}", self.gvk, self.name)
        }
    }
}

/// Resource version - maps to jj commit ID
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceVersion(pub String);

impl ResourceVersion {
    pub fn new(version: impl Into<String>) -> Self {
        Self(version.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ResourceVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for ResourceVersion {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ResourceVersion {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gvk_from_api_version() {
        let gvk = GroupVersionKind::from_api_version_kind("v1", "Pod");
        assert_eq!(gvk.group, "");
        assert_eq!(gvk.version, "v1");
        assert_eq!(gvk.kind, "Pod");
        assert_eq!(gvk.api_version(), "v1");

        let gvk = GroupVersionKind::from_api_version_kind("apps/v1", "Deployment");
        assert_eq!(gvk.group, "apps");
        assert_eq!(gvk.version, "v1");
        assert_eq!(gvk.kind, "Deployment");
        assert_eq!(gvk.api_version(), "apps/v1");
    }

    #[test]
    fn test_gvk_resource_name() {
        let gvk = GroupVersionKind::from_api_version_kind("v1", "Pod");
        assert_eq!(gvk.resource_name(), "pods");

        let gvk = GroupVersionKind::from_api_version_kind("apps/v1", "Deployment");
        assert_eq!(gvk.resource_name(), "deployments");
    }

    #[test]
    fn test_resource_key_storage_key() {
        let gvk = GroupVersionKind::from_api_version_kind("v1", "Pod");
        let key = ResourceKey::new(gvk, "default", "nginx");
        assert_eq!(key.storage_key(), "v1/Pod/default/nginx");

        let gvk = GroupVersionKind::from_api_version_kind("v1", "Node");
        let key = ResourceKey::cluster_scoped(gvk, "node-1");
        assert_eq!(key.storage_key(), "v1/Node/node-1");
    }

    #[test]
    fn test_resource_key_api_path() {
        let gvk = GroupVersionKind::from_api_version_kind("v1", "Pod");
        let key = ResourceKey::new(gvk, "default", "nginx");
        assert_eq!(key.api_path(), "/api/v1/namespaces/default/pods/nginx");

        let gvk = GroupVersionKind::from_api_version_kind("v1", "Node");
        let key = ResourceKey::cluster_scoped(gvk, "node-1");
        assert_eq!(key.api_path(), "/api/v1/nodes/node-1");
    }
}
