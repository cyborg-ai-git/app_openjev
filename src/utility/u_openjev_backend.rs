#[cfg(feature = "local")]
#[derive(Clone)]
pub struct UOpenjevLocalConfig {
    pub directory: std::path::PathBuf,
    pub device: String,
    pub precision: String,
}

#[cfg(feature = "local")]
impl UOpenjevLocalConfig {
    pub fn device_name(&self) -> &str {
        if self.device == "auto" {
            if cfg!(all(feature = "metal", target_os = "macos")) {
                "metal"
            } else {
                "cpu"
            }
        } else {
            &self.device
        }
    }

    pub fn load(&self) -> anyhow::Result<crate::local::LocalModel> {
        let device = crate::local::device(self.device_name())?;
        if self.precision == "auto" {
            crate::local::LocalModel::load(&self.directory, device)
        } else {
            crate::local::LocalModel::load_with_options(
                &self.directory,
                device,
                crate::local::LoadOptions {
                    dtype: match self.precision.as_str() {
                        "f16" => candle_core::DType::F16,
                        "f32" => candle_core::DType::F32,
                        _ => anyhow::bail!("Unknown precision: choose auto, f32, or f16"),
                    },
                    reference_encoder: false,
                },
            )
        }
    }
}
