use std::env;
use std::fmt::format;
use dotenv::dotenv;
use sqlx::{SqlitePool, Pool, Sqlite, error};
use serenity::async_trait;
use serenity::model::channel::Message as DiscordMessage;
use serenity::model::gateway::Ready;
use serenity::prelude::*;

#[derive(Debug, Clone)]
struct DbMessage {
    id: i64,
    guild_id: String,
    channel_id: String,
    message_id: String,
    author_id: String,
    content: String,
    image_hash: Option<String>,
}

struct Handler {
    db_pool: Pool<Sqlite>,
}

impl Handler {
    fn new(db_pool: Pool<Sqlite>) -> Handler {
        Handler {
            db_pool,
        }
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: DiscordMessage) {
        if msg.author.bot { return };

        let allowed_channels = &env::var("DISCORD_LISTENING_CHANNEL_IDS").unwrap();
        let mut allowed_channels = allowed_channels.split(",");
        let channel_id = msg.channel_id.to_string();
        if !allowed_channels.any(|c| c == channel_id) {
            return;
        }

        let repo = MessageRepository::new(self.db_pool.clone());

        let content = &msg.content;

        if let Some(message) = repo.find_duplicate(&content).await {
            println!("found duplicate: {:?}", message);
            let message_url = format!("https://discord.com/channels/{}/{}/{}", message.guild_id, message.channel_id, message.message_id);
            let reply = format!("Duplicate of {}", message_url);
            msg.reply(&ctx.http, reply).await.expect("Failed to reply");
        }

        let guild_id = match msg.guild_id {
            Some(guild_id) => guild_id.to_string(),
            None => return,
        };
        let message_id = msg.id.to_string();
        let author_id = msg.author.id.to_string();

        if author_id == "141320575132893184" {
            let message_url = format!("https://discord.com/channels/{}/{}/{}", message.guild_id, message.channel_id, message.message_id);
            msg.reply(&ctx.http, "retard detected").await.expect("Failed to reply");
        }
        
        repo.create(guild_id, channel_id, message_id, author_id, content.clone()).await.expect("Failed to create message");
    }

    async fn ready(&self, _: Context, ready: Ready) {
        println!("Connected to discord as {}", ready.user.name);
    }
}

struct MessageRepository {
    db_pool: Pool<Sqlite>,
}

impl MessageRepository {
    fn new(db_pool: Pool<Sqlite>) -> MessageRepository {
        MessageRepository {
            db_pool
        }
    }

    async fn create(&self, guild_id: String, channel_id: String, message_id: String, author_id: String, content: String) -> Result<DbMessage, anyhow::Error> {
        let pool = &self.db_pool;
        let rowid = sqlx::query!(
            r#"
                INSERT INTO messages (guild_id, channel_id, message_id, author_id, content)
                VALUES (
                    ?1,
                    ?2,
                    ?3,
                    ?4,
                    ?5
                );
            "#,
            guild_id,
            channel_id,
            message_id,
            author_id,
            content
        )
        .execute(pool)
        .await?
        .last_insert_rowid();

        let messages = sqlx::query_as!(DbMessage, "select * from messages where rowid = ?1", rowid)
            .fetch_all(pool)
            .await?;

        if let Some(message) = messages.first() {
            Ok(message.clone())
        } else {
            panic!("Cant find newly created rowid");
        }
    }

    async fn find_duplicate(&self, content: &str) -> Option<DbMessage> {
        if content.len() < 8 { return None }

        let messages = sqlx::query_as!(DbMessage, "select * from messages where content = ?1 order by id asc", content)
            .fetch_all(&self.db_pool)
            .await.expect("failed to hit db");

        if let Some(message) = messages.first() {
            Some(message.clone())
        } else {
            None
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    dotenv().ok();
    let pool = start_db().await?;
    let _discord = start_discord(pool).await?;
    Ok(())
}

async fn start_db() -> Result<Pool<Sqlite>, anyhow::Error> {
    let pool = SqlitePool::connect(&env::var("DATABASE_URL")?).await?;

    sqlx::migrate!()
        .run(&pool)
        .await?;

    let messages = sqlx::query_as!(DbMessage, "select * from messages")
        .fetch_all(&pool)
        .await.unwrap();

    for message in messages {
        println!("{:?}", message)
    }

    Ok(pool)
}

async fn start_discord(db_pool: Pool<Sqlite>) -> Result<Client, anyhow::Error> {
    let token = env::var("DISCORD_TOKEN").expect("Set DISCORD_TOKEN");

    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;  


    let handler: Handler = Handler::new(db_pool);

    let mut client = Client::builder(&token, intents)
        .event_handler(handler).await.expect("Err creating discord");

    if let Err(why) = client.start().await {
        return Err(why.into());
    }

    Ok(client)
}
