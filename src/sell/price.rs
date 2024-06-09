use std::error::Error;

pub async fn get_current_sol_price() -> Result<f64, Box<dyn Error>> {
    let url =
        "https://public-api.birdeye.so/defi/price?address=So11111111111111111111111111111111111111112";

    let birdeye_api_key = std::env
        ::var("BIRDEYE_API")
        .expect("You must set the RPC_URL environment variable!");
    // Make GET request to the API endpoint with API key in header
    let response = reqwest::Client
        ::new()
        .get(url)
        .header("X-API-KEY", birdeye_api_key)
        .send().await?;

    // Check if response is successful
    if response.status().is_success() {
        // Parse the JSON response to extract the SOL price
        let sol_price_json: serde_json::Value = response.json().await?;
        println!("SOL price JSON: {:?}", sol_price_json);
        let sol_price_usd: f64 = sol_price_json["data"]["value"].as_f64().unwrap_or(0.0);
        Ok(sol_price_usd)
    } else {
        Err("Failed to fetch SOL price".into()) // Convert string to boxed Error trait object
    }
}
