use solana_transaction_status::{ UiInnerInstructions, UiInstruction, UiParsedInstruction };
use serde_json::Value;
use mpl_token_metadata::accounts::Metadata;
pub use mpl_token_metadata::ID;
use super::mongo;
use mongo::TokenMetadata;
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

pub fn get_second_instruction_amount(
    inner_instructions: &Vec<UiInnerInstructions>
) -> Option<String> {
    // Iterate over each UiInnerInstructions
    for inner in inner_instructions {
        // Check if there are at least two instructions
        if inner.instructions.len() >= 2 {
            // Get the second instruction
            if let UiInstruction::Parsed(parsed_instruction) = &inner.instructions[1] {
                if let UiParsedInstruction::Parsed(instruct) = parsed_instruction {
                    // Extract the amount from the parsed data
                    if let Some(info) = instruct.parsed.get("info") {
                        if let Some(amount) = info.get("amount") {
                            if let Value::String(amount_str) = amount {
                                return Some(amount_str.clone());
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

pub async fn get_token_metadata(
    mint_address: &str,
    balance: f64,
    client: &RpcClient
) -> Result<TokenMetadata, Box<dyn std::error::Error>> {
    let metadata_program_id = &ID;
    let token_mint_address = Pubkey::from_str(mint_address)?;

    let (metadata_account_address, _) = Pubkey::find_program_address(
        &[b"metadata", metadata_program_id.as_ref(), token_mint_address.as_ref()],
        &metadata_program_id
    );

    // Attempt to fetch and deserialize the account data for the metadata account
    let account_data_result = client.get_account_data(&metadata_account_address)?;
    let metadata: Metadata = Metadata::from_bytes(&account_data_result)?;

    // Remove trailing null characters from name and symbol
    let name = metadata.name.trim_matches(char::from(0)).to_string();
    let symbol = metadata.symbol.trim_matches(char::from(0)).to_string();

    // Fetch URI to get additional metadata
    let uri = metadata.uri.trim_matches(char::from(0)).to_string();
    let uri_response = reqwest::get(&uri).await?;
    let metadata_json: serde_json::Value = uri_response.json().await?;

    // Extract required fields from the JSON
    let description = metadata_json["description"].as_str().unwrap_or_default().to_string();
    let image = metadata_json["image"].as_str().unwrap_or_default().to_string();
    let twitter = metadata_json["twitter"].as_str().unwrap_or_default().to_string();
    let created_on = metadata_json["createdOn"].as_str().unwrap_or_default().to_string();

    // Construct TokenMetadata object
    let token_metadata = TokenMetadata {
        name,
        symbol,
        balance,
        mint: mint_address.to_string(),
        description,
        image,
        twitter,
        created_on,
    };
    Ok(token_metadata)
}
