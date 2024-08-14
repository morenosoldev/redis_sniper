use mongodb::{ Client, options::ClientOptions, bson::doc, bson::Document, Collection };
use mongodb::error::Error as MongoError;
use serde::Serialize;
use serde::Deserialize;
use mongodb::bson::DateTime;
use futures::stream::TryStreamExt;

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
    pub token_metadata: Option<TokenMetadata>,
    pub profit: f64,
    pub profit_usd: f64,
    pub profit_percentage: f64,
    pub created_at: DateTime,
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
    pub stop_loss_triggered: bool,
    pub initial_investment: f64,
    pub stop_loss_at_breakeven: bool,
    pub taken_out: f64,
    pub remaining: f64,
    pub token_metadata: Option<TokenMetadata>,
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

    pub async fn update_buy_transaction(
        &self,
        buy_transaction: &BuyTransaction
    ) -> Result<(), MongoError> {
        let db = self.client.database("solsniper");
        let collection: Collection<Document> = db.collection("buy_transactions"); // Replace with your collection name

        let filter =
            doc! {
            "transaction_signature": &buy_transaction.transaction_signature
        };

        let update =
            doc! {
            "$set": {
                "amount": buy_transaction.amount,
                "sol_amount": buy_transaction.sol_amount,
                "sol_price": buy_transaction.sol_price,
                "usd_amount": buy_transaction.usd_amount,
                "entry_price": buy_transaction.entry_price,
                "fee_sol": buy_transaction.fee_sol,
                "fee_usd": buy_transaction.fee_usd,
                "created_at": buy_transaction.created_at,
                "transaction_type": bson::to_bson(&buy_transaction.transaction_type)?
            }
        };

        // Check if amount or sol_amount is 0 or 0.0
        if buy_transaction.amount == 0.0 {
            self.update_token_metadata_sold_field(
                &buy_transaction.token_info.base_mint,
                "solsniper",
                "tokens"
            ).await?;
        }

        collection.update_one(filter, update, None).await?;

        Ok(())
    }

    pub async fn is_token_sold(
        &self,
        db_name: &str,
        collection_name: &str,
        mint_address: &str
    ) -> Result<bool, MongoError> {
        let my_coll: Collection<Document> = self.client
            .database(db_name)
            .collection(collection_name);

        let filter = doc! { "token_metadata.mint": mint_address };

        // Find the document with the specific mint address
        let mut cursor = my_coll.find(filter, None).await?;
        let document = match cursor.try_next().await? {
            Some(doc) => doc,
            None => {
                return Ok(false);
            }
        };

        // Check if the "sold" field is true
        if let Some(sold) = document.get("sold") {
            if let bson::Bson::Boolean(sold) = sold {
                return Ok(*sold); // Dereferencing the borrow
            }
        }
        Ok(false)
    }

    pub async fn get_buy_transaction_from_token(
        &self,
        token_mint: &str,
        db_name: &str,
        collection_name: &str
    ) -> Result<BuyTransaction, MongoError> {
        let db = self.client.database(db_name);
        let collection: Collection<Document> = db.collection(collection_name);

        let filter = doc! {
            "token_info.base_mint": token_mint
        };

        let document = match collection.find_one(filter, None).await {
            Ok(doc) => doc,
            Err(e) => {
                return Err(e);
            }
        };

        match document {
            Some(doc) => {
                match bson::from_document(doc) {
                    Ok(buy_transaction) => Ok(buy_transaction),
                    Err(e) => {
                        Err(MongoError::from(e)) // You might want to adjust this to match your error type
                    }
                }
            }
            None => {
                Ok(BuyTransaction {
                    transaction_signature: "".to_string(),
                    token_info: TokenInfo {
                        base_mint: "".to_string(),
                        quote_mint: "".to_string(),
                        base_vault: "".to_string(),
                        quote_vault: "".to_string(),
                    },
                    amount: 0.0,
                    sol_amount: 0.0,
                    sol_price: 0.0,
                    entry_price: 0.0,
                    usd_amount: 0.0,
                    fee_sol: 0.0,
                    fee_usd: 0.0,
                    created_at: DateTime::now(),
                    transaction_type: TransactionType::LongTermHold,
                })
            }
        }
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

        // Perform the update operation
        match collection.update_one(filter.clone(), update, None).await {
            Ok(_update_result) => { Ok(()) }
            Err(e) => { Err(e) }
        }
    }

    pub async fn fetch_trade_state(&self, token_mint: &str) -> Result<TradeState, MongoError> {
        let db = self.client.database("trading"); // Replace with your database name
        let collection: Collection<Document> = db.collection("trade_states"); // Replace with your collection name

        let filter = doc! {
            "token_mint": token_mint
        };

        let document = collection.find_one(filter, None).await?;

        match document {
            Some(doc) => {
                match bson::from_document::<TradeState>(doc) {
                    Ok(trade_state) => Ok(trade_state),
                    Err(e) => { Err(MongoError::from(e)) }
                }
            }
            None => {
                // If no document is found, return a default TradeState
                // You might want to return an error or handle it differently
                Ok(TradeState {
                    token_mint: token_mint.to_string(),
                    entry_price: 0.0,
                    ath_50_percent_triggered: false,
                    initial_investment_taken: false,
                    profit_taking_count: 0,
                    last_profit_taking_time: None,
                    last_profit_percentage: 0.0,
                    stop_loss_triggered: false,
                    initial_investment: 0.0,
                    stop_loss_at_breakeven: false,
                    token_metadata: None,
                    taken_out: 0.0,
                    remaining: 0.0,
                })
            }
        }
    }

    pub async fn update_trade_state(&self, trade_state: &TradeState) -> Result<(), MongoError> {
        let db = self.client.database("trading"); // Replace with your database name
        let collection: Collection<Document> = db.collection("trade_states"); // Replace with your collection name

        let filter = doc! {
            "token_mint": &trade_state.token_mint
        };

        let update =
            doc! {
            "$set": {
                "entry_price": trade_state.entry_price,
                "initial_investment_taken": trade_state.initial_investment_taken,
                "ath_50_percent_triggered": trade_state.ath_50_percent_triggered,
                "profit_taking_count": trade_state.profit_taking_count,
                "last_profit_taking_time": trade_state.last_profit_taking_time,
                "last_profit_percentage": trade_state.last_profit_percentage,
                "stop_loss_triggered": trade_state.stop_loss_triggered,
                "initial_investment": trade_state.initial_investment,
                "taken_out": trade_state.taken_out,
                "remaining": trade_state.remaining,

            }
        };

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
