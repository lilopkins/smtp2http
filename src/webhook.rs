use color_eyre::Result;

use crate::settings::Settings;

pub async fn send(settings: &Settings, client: &reqwest::Client, raw_email: &str) -> Result<()> {
    let res = client
        .post(&settings.webhook)
        .header("Content-Type", "message/rfc822")
        .body(raw_email.to_string())
        .send()
        .await?;

    if res.status().is_success() {
        println!("[Webhook] Successfully forwarded email to Bun endpoint.");
    } else {
        eprintln!("[Webhook] Endpoint returned HTTP status: {}", res.status());
    }

    Ok(())
}
