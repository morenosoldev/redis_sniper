use std::error::Error;

pub async fn get_current_sol_price() -> Result<f64, Box<dyn Error>> {
    let url = "https://api.coingecko.com/api/v3/simple/price?ids=solana&vs_currencies=usd";
    let response = reqwest::get(url).await?;

    let json: serde_json::Value = response.json().await?;
    let price = json["solana"]["usd"].as_f64().unwrap_or(0.0);

    Ok(price)
}
