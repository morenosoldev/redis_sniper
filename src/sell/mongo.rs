use mongodb::{ Client, options::ClientOptions, bson::doc, bson::Document, Collection };
use mongodb::error::Error as MongoError;
use serde::Serialize;
use serde::Deserialize;
use mongodb::bson::DateTime;

pub struct MongoHandler {
    client: Client,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TokenMetadata {
    pub name: String,
    pub symbol: String,
    pub balance: f64,
    pub mint: String,
    pub description: String,
    pub image: String,
    pub twitter: String,
    pub created_on: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum TransactionType {
    LongTermHold,
    ShortTermSell,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BuyTransaction {
    pub transaction_signature: String,
    pub token_info: TokenInfo,
    pub amount: f64,
    pub sol_amount: f64,
    pub sol_price: f64,
    pub entry_price: f64,
    pub usd_amount: f64,
    pub fee_sol: f64,
    pub fee_usd: f64,
    pub created_at: DateTime,
    pub transaction_type: TransactionType,
}

#[derive(Debug, Serialize, Clone, Deserialize)]
pub struct SellTransaction {
    pub transaction_signature: String,
    pub token_info: TokenInfo,
    pub amount: f64,
    pub sol_amount: f64,
    pub sol_price: f64,
    pub sell_price: f64,
    pub fee_sol: f64,
    pub fee_usd: f64,
    pub entry_price: f64,
    pub token_metadata: TokenMetadata,
    pub profit: f64,
    pub profit_usd: f64,
    pub profit_percentage: f64,
    pub created_at: DateTime,
}

#[derive(Debug, Serialize, Deserialize, Clone)] // Add `Clone` to derive
pub struct TokenInfo {
    pub base_mint: String,
    pub quote_mint: String,
    pub base_vault: String,
    pub quote_vault: String,
}

impl MongoHandler {
    pub async fn new() -> Result<Self, MongoError> {
        // Load the MongoDB connection string from an environment variable
        let client_uri = std::env
            ::var("MONGODB_URI")
            .expect("You must set the MONGODB_URI environment variable!");

        // Parse the client options
        let options = ClientOptions::parse(&client_uri).await?;
        let client = Client::with_options(options)?;

        Ok(Self { client })
    }

    pub async fn update_token_metadata_sold_field(
        &self,
        mint: &str,
        db_name: &str,
        collection_name: &str
    ) -> Result<(), MongoError> {
        let db = self.client.database(db_name);
        let collection: Collection<Document> = db.collection(collection_name);

        // Define the filter to find the document with the given ObjectId
        let filter = doc! {
            "token_metadata.mint": mint
        };

        // Define the update operation to set the "sold" field to true
        let update = doc! {
            "$set": {
                "sold": true
            }
        };

        // Update the document matching the filter
        collection.update_one(filter, update, None).await?;

        Ok(())
    }

    pub async fn store_sell_transaction_info(
        &self,
        transaction: SellTransaction,
        db_name: &str,
        collection_name: &str
    ) -> Result<(), MongoError> {
        let db = self.client.database(db_name);
        let collection: Collection<Document> = db.collection(collection_name);

        // Convert the entire token_metadata into a Document
        let document = bson::to_document(&transaction)?;

        collection.insert_one(document, None).await?;

        Ok(())
    }
}
