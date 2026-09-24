use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EnumOpenjevBackend {
    Remote,
    Local,
    Demo,
}

impl EnumOpenjevBackend {
    pub fn label(self) -> &'static str {
        match self {
            Self::Remote => "REMOTE",
            Self::Local => "LOCAL",
            Self::Demo => "DEMO",
        }
    }
}
