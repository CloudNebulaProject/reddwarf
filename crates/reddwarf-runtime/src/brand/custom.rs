use crate::types::ContainerProcess;

/// Generate a process supervisor configuration for the reddwarf brand
///
/// This produces a configuration format that the reddwarf brand's process
/// supervisor will consume to start and manage container processes within the zone.
pub fn generate_supervisor_config(processes: &[ContainerProcess]) -> String {
    let mut lines = Vec::new();

    for proc in processes {
        lines.push(format!("[process.{}]", proc.name));
        lines.push(format!(
            "command = {}",
            proc.command
                .iter()
                .map(|s| format!("\"{}\"", s))
                .collect::<Vec<_>>()
                .join(" ")
        ));
        if let Some(ref dir) = proc.working_dir {
            lines.push(format!("working_dir = \"{}\"", dir));
        }
        for (key, value) in &proc.env {
            lines.push(format!("env.{} = \"{}\"", key, value));
        }
        lines.push(String::new());
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_supervisor_config() {
        let processes = vec![
            ContainerProcess {
                name: "web".to_string(),
                command: vec!["/usr/bin/node".to_string(), "server.js".to_string()],
                working_dir: Some("/app".to_string()),
                env: vec![("PORT".to_string(), "3000".to_string())],
            },
            ContainerProcess {
                name: "sidecar".to_string(),
                command: vec!["/usr/bin/envoy".to_string()],
                working_dir: None,
                env: vec![],
            },
        ];

        let config = generate_supervisor_config(&processes);
        assert!(config.contains("[process.web]"));
        assert!(config.contains("command = \"/usr/bin/node\" \"server.js\""));
        assert!(config.contains("working_dir = \"/app\""));
        assert!(config.contains("env.PORT = \"3000\""));
        assert!(config.contains("[process.sidecar]"));
    }
}
