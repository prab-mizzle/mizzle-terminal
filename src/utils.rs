use tokio::{io as tio, process::Command};

use crate::obj::Claims;

pub async fn get_program_version(program: &str) -> tio::Result<Vec<u8>> {
    let output = Command::new(program).arg("--version").output().await?;
    Ok(output.stdout)
}

pub async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            println!("+ Shutdown in recv: ");
        },
        _ = terminate => {
            println!("+ Shutdown recv: ");
        },
    }
}

pub fn jwt_token_generator(expiration: u64, key: &str) -> Result<String, Box<dyn std::error::Error>> {

    // let claim = Claims { 
    //     token_string: "eyJhbGciOiJIUzI1…Qhkntf78O9kF71F8".to_string(),
    //     user_claim: "username".to_string(),
    //     id: "ggicci".to_string(),
    //     // exp: expiration,
    // };

    // let claim = Claims {
    //     exp: 1910000000, // Example timestamp
    //     jti: "123e4567-e89b-12d3-a456-426614174000".to_string(),
    //     sub: "9876543210987654".to_string(),
    //     iss: "https://api.dummy.com".to_string(),
    //     aud: vec!["https://api.client.io".to_string()],
    //     username: "dummy_user".to_string(),
    // };

    // let token = match jsonwebtoken::encode(
    //     &jsonwebtoken::Header::default(), 
    //     &claim, 
    //     &jsonwebtoken::EncodingKey::from_secret(key.as_bytes())
    // ) {
    //     Ok(t) => t,
    //     Err(_) => panic!(), // in practice you would return the error
    // };

    //todo: change this to return a token with session value of 30mins
    let token = r#"eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJleHAiOjk5NTU4OTI2NzAsImp0aSI6IjgyMjk0YTYzLTk2NjAtNGM2Mi1hOGE4LTVhNjI2NWVmY2Q0ZSIsInN1YiI6IjM0MDYzMjc5NjM1MTY5MzIiLCJpc3MiOiJodHRwczovL2FwaS5leGFtcGxlLmNvbSIsImF1ZCI6WyJodHRwczovL2FwaS5leGFtcGxlLmlvIl0sInVzZXJuYW1lIjoiZ2dpY2NpIn0.O8kvRO9y6xQO3AymqdFE7DDqLRBQhkntf78O9kF71F8"#;

    Ok(token.to_string())
}