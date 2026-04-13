#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreSurface {
    CliLocal,
    TuiLocal,
    WebUiLocal,
}

impl CoreSurface {
    pub const fn id(self) -> &'static str {
        match self {
            Self::CliLocal => "cli.local",
            Self::TuiLocal => "tui.local",
            Self::WebUiLocal => "webui.local",
        }
    }
}

pub fn is_core_surface_id(surface_id: &str) -> bool {
    matches!(surface_id, "cli.local" | "tui.local" | "webui.local")
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{domain::PluginCapability, plugins::known_plugin_manifests};

    #[test]
    fn core_surface_ids_are_reserved_and_channels_stay_plugins() {
        assert!(is_core_surface_id(CoreSurface::CliLocal.id()));
        assert!(is_core_surface_id(CoreSurface::TuiLocal.id()));
        assert!(is_core_surface_id(CoreSurface::WebUiLocal.id()));

        let manifests = known_plugin_manifests();
        assert!(manifests
            .iter()
            .any(|manifest| manifest.id == "channel.telegram"));
        assert!(manifests.iter().any(|manifest| {
            manifest
                .capabilities
                .iter()
                .any(|capability| matches!(capability, PluginCapability::Channel(_)))
        }));
        assert!(manifests.iter().all(|manifest| {
            manifest
                .capabilities
                .iter()
                .all(|capability| !matches!(capability, PluginCapability::Ui(_)))
        }));
    }
}
