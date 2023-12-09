/// Begins with an underscore because this must be the first file to be called
mod utils;

use crunchyroll_rs::Crunchyroll;
use std::env;

#[tokio::test]
async fn login_with_credentials() {
    let email = env::var("EMAIL").expect("'EMAIL' environment variable not found");
    let password = env::var("PASSWORD").expect("'PASSWORD' environment variable not found");

    let crunchy = Crunchyroll::builder()
        .login_with_credentials(email, password)
        .await;

    assert_result!(crunchy);

    if !utils::session::has_session() {
        utils::session::set_session(crunchy.unwrap()).await.unwrap()
    }
}

#[tokio::test]
async fn login_with_refresh_token() {
    let refresh_token =
        env::var("REFRESH_TOKEN").expect("'REFRESH_TOKEN' environment variable not found");

    let crunchy = Crunchyroll::builder()
        .login_with_refresh_token(refresh_token)
        .await;

    assert_result!(crunchy);

    if !utils::session::has_session() {
        utils::session::set_session(crunchy.unwrap()).await.unwrap()
    }
}

#[tokio::test]
async fn login_with_etp_rt() {
    let etp_rt = env::var("ETP_RT").expect("'ETP_RT' environment variable not found");

    let crunchy = Crunchyroll::builder().login_with_etp_rt(etp_rt).await;

    assert_result!(crunchy);

    if !utils::session::has_session() {
        utils::session::set_session(crunchy.unwrap()).await.unwrap()
    }
}

#[tokio::test]
async fn login_anonymously() {
    let crunchy = Crunchyroll::builder().login_anonymously().await;

    assert_result!(crunchy);

    if !utils::session::has_session() {
        utils::session::set_session(crunchy.unwrap()).await.unwrap()
    }
}
