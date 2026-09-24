use std::{io::Write, sync::Mutex, time::Duration};

use evo_core_entity::UId;
use evo_core_env::UEnv;
use openjev::{Request, UOpenjevCredentials, app::demo_response};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, method, path},
};

struct CapturingLogger(Mutex<String>);
static LOGGER: CapturingLogger = CapturingLogger(Mutex::new(String::new()));
impl log::Log for CapturingLogger {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }
    fn log(&self, record: &log::Record<'_>) {
        self.0.lock().unwrap().push_str(&record.args().to_string());
    }
    fn flush(&self) {}
}

// One test owns the process-global Evo map and log level throughout the fixture.
#[test]
fn evo_credentials_are_validated_redacted_and_sent_correctly() {
    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let id = UId::id_str("TYPESAFE_TOKEN");
    let mut config = tempfile::NamedTempFile::new().unwrap();
    let config_path = config.path().to_str().unwrap().to_owned();
    let fixtures = [
        "[TYPESAFE_TOKEN]\nvalue = 'synthetic-private-token'\n",
        "[TYPESAFE_TOKEN]\nenabled = true\nvalue = 'synthetic-private-token'\n",
    ];
    for text in fixtures {
        config.as_file_mut().set_len(0).unwrap();
        use std::io::{Seek, SeekFrom};
        config.seek(SeekFrom::Start(0)).unwrap();
        config.write_all(text.as_bytes()).unwrap();
        let credentials = UOpenjevCredentials::from_path_config(&config_path).unwrap();
        assert!(format!("{credentials:?}").contains("REDACTED"));
        assert!(!format!("{credentials:?}").contains("synthetic-private-token"));
        assert!(UEnv::get_env_value(&id).is_err());
        assert_eq!(log::max_level(), log::LevelFilter::Trace);
        assert!(!LOGGER.0.lock().unwrap().contains("synthetic-private-token"));
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let server = MockServer::start().await;
            let request: Request =
                serde_json::from_str(include_str!("../examples/triage.json")).unwrap();
            Mock::given(method("POST"))
                .and(path("/v1/systemone"))
                .and(header("Authorization", "Bearer synthetic-private-token"))
                .respond_with(ResponseTemplate::new(200).set_body_json(demo_response(&request)))
                .expect(1)
                .mount(&server)
                .await;
            credentials
                .client(&server.uri(), Duration::from_secs(3))
                .unwrap()
                .evaluate(&request)
                .await
                .unwrap();
        });
        LOGGER.0.lock().unwrap().clear();
    }
    let invalid = [
        "",
        "[OTHER]\nvalue = 'synthetic-private-token'",
        "[TYPESAFE_TOKEN]\nvalue = ''",
        "[TYPESAFE_TOKEN]\nvalue = 123",
        "[TYPESAFE_TOKEN]\nenabled = false\nvalue = 'synthetic-private-token'",
        "[TYPESAFE_TOKEN]\nenabled = 'yes'\nvalue = 'synthetic-private-token'",
        "[TYPESAFE_TOKEN]\nvalue = 'synthetic-private-token with spaces'",
        "[TYPESAFE_TOKEN]\nvalue = \"synthetic-private-token\\r\\n\"",
        "[TYPESAFE_TOKEN]\nvalue = 'synthetic-private-token'\n[ZZZ]\nvalue = 123",
        "[TYPESAFE_TOKEN]\nvalue = 'synthetic-private-token'\n[broken",
    ];
    for text in invalid {
        std::fs::write(&config_path, text).unwrap();
        let error = UOpenjevCredentials::from_path_config(&config_path).unwrap_err();
        assert!(!format!("{error:?}").contains("synthetic-private-token"));
        assert!(UEnv::get_env_value(&id).is_err());
        assert_eq!(log::max_level(), log::LevelFilter::Trace);
        assert!(!LOGGER.0.lock().unwrap().contains("synthetic-private-token"));
    }
    std::fs::write(&config_path, "x".repeat(1024 * 1024 + 1)).unwrap();
    assert!(UOpenjevCredentials::from_path_config(&config_path).is_err());
    config.close().unwrap();
    assert!(UOpenjevCredentials::from_path_config(&config_path).is_err());
    assert_eq!(log::max_level(), log::LevelFilter::Trace);
    assert!(!LOGGER.0.lock().unwrap().contains("synthetic-private-token"));
}
