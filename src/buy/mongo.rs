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
    pub transaction_type: TransactionType,
    pub token_info: TokenInfo,
    pub initial_amount: f64,
    pub highest_profit_percentage: f64,
    pub amount: f64,
    pub sol_amount: f64,
    pub sol_price: f64,
    pub usd_amount: f64,
    pub entry_price: f64,
    pub fee_sol: f64,
    pub fee_usd: f64,
    pub token_metadata: TokenMetadata,
    pub created_at: DateTime,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SellTransaction {
    pub transaction_signature: String,
    pub token_info: TokenInfo,
    pub amount: f64,
    pub sol_amount: f64,
    pub sol_price: f64,
    pub sell_price: f64,
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

#[derive(Serialize, Deserialize, Debug)]
pub struct TradeState {
    pub token_mint: String,
    pub entry_price: f64,
    pub initial_investment_taken: bool,
    pub ath_50_percent_triggered: bool,
    pub profit_taking_count: i32,
    pub last_profit_taking_time: Option<DateTime>,
    pub last_profit_percentage: f64,
    pub stop_loss_at_breakeven: bool,
    pub stop_loss_triggered: bool,
    pub initial_investment: f64,
    pub taken_out: f64,
    pub remaining: f64,
    pub token_metadata: Option<TokenMetadata>,
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

    pub async fn create_trade_state(&self, trade_state: &TradeState) -> Result<(), MongoError> {
        let db = self.client.database("trading"); // Replace with your database name
        let collection: Collection<Document> = db.collection("trade_states"); // Replace with your collection name

        // Convert TradeState to BSON document
        let doc = bson::to_document(trade_state)?;

        // Insert the document
        collection.insert_one(doc, None).await?;

        Ok(())
    }

    pub async fn store_token(
        &self,
        token_metadata: TokenMetadata,
        db_name: &str,
        collection_name: &str,
        sol_amount: f64
    ) -> Result<(), MongoError> {
        let db = self.client.database(db_name);
        let collection = db.collection::<Document>(collection_name);

        let filter = doc! { "token_metadata.mint": &token_metadata.mint };
        let existing_document = collection.find_one(filter, None).await?;

        if existing_document.is_none() {
            // Convert the balance to BSON
            let balance_bson = bson::to_bson(&token_metadata.balance)?;

            // Create the document to be inserted
            let document =
                doc! {
                "sold": false,
                "token_metadata": {
                    "name": &token_metadata.name,
                    "symbol": &token_metadata.symbol,
                    "mint": &token_metadata.mint,
                    "balance": balance_bson,
                    "description": &token_metadata.description,
                    "image": &token_metadata.image,
                    "twitter": &token_metadata.twitter,
                    "created_on": &token_metadata.created_on,
                }
            };

            // Insert the document
            collection.insert_one(document, None).await?;

            let new_trade_state = TradeState {
                token_mint: token_metadata.mint.clone(),
                entry_price: 0.0,
                ath_50_percent_triggered: false,
                initial_investment_taken: false,
                profit_taking_count: 0,
                last_profit_taking_time: None,
                last_profit_percentage: 0.0,
                stop_loss_triggered: false,
                stop_loss_at_breakeven: false,
                initial_investment: sol_amount,
                token_metadata: Some(token_metadata),
                taken_out: 0.0,
                remaining: 0.0,
            };

            self.create_trade_state(&new_trade_state).await?;
        }

        Ok(())
    }

    pub async fn store_buy_transaction_info(
        &self,
        transaction: BuyTransaction,
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
