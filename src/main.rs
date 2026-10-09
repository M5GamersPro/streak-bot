use poise::serenity_prelude as serenity;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::sync::Arc;
use tokio::sync::Mutex;
use chrono::{DateTime, Utc, Duration, Datelike};
use serde::{Serialize, Deserialize};

const DATA_FILE: &str = "streaks.json";

#[derive(Serialize, Deserialize, Clone, Debug)]
struct UserProfile {
    username: String,
    streak_count: u32,
    last_check_in: DateTime<Utc>,
}

struct Data {
    profiles: Arc<Mutex<HashMap<String, UserProfile>>>,
}

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

// Helper function to load data safely from local storage
fn load_data() -> HashMap<String, UserProfile> {
    if let Ok(mut file) = File::open(DATA_FILE) {
        let mut contents = String::new();
        if file.read_to_string(&mut contents).is_ok() {
            if let Ok(data) = serde_json::from_str(&contents) {
                return data;
            }
        }
    }
    HashMap::new()
}

// Helper function to save data safely to local storage
fn save_data(data: &HashMap<String, UserProfile>) {
    if let Ok(json) = serde_json::to_string_pretty(data) {
        if let Ok(mut file) = File::create(DATA_FILE) {
            let _ = file.write_all(json.as_bytes());
        }
    }
}

/// Check-in to claim and advance your daily streak status!
#[poise::command(slash_command, prefix_command)]
async fn streak(ctx: Context<'_>) -> Result<(), Error> {
    let user_id = ctx.author().id.to_string();
    let user_name = ctx.author().name.clone();
    let now = Utc::now();
    
    let mut lock = ctx.data().profiles.lock().await;
    
    let mut message = String::new();
    let mut color = serenity::Color::from_rgb(52, 152, 219);

    if let Some(profile) = lock.get_mut(&user_id) {
        // Keep their username updated in case they changed it
        profile.username = user_name.clone();

        let last = profile.last_check_in;
        let time_since = now.signed_duration_since(last);

        // 1. Same Calendar Day Check
        if now.date_naive() == last.date_naive() {
            let tomorrow_midnight = (now + Duration::days(1)).date_naive().and_hms_opt(0, 0, 0).unwrap();
            let tomorrow_utc = DateTime::<Utc>::from_naive_utc_and_offset(tomorrow_midnight, Utc);
            let wait_time = tomorrow_utc.signed_duration_since(now);

            color = serenity::Color::from_rgb(230, 126, 34); // Orange
            message = format!(
                "⚠️ **{}**, you already checked in today!\nYour streak resets if you don't check in tomorrow. Next window opens in **{}h {}m**.",
                user_name,
                wait_time.num_hours(),
                wait_time.num_minutes() % 60
            );
        // 2. Next Calendar Day Check (Valid window)
        } else if time_since <= Duration::hours(48) {
            profile.streak_count += 1;
            profile.last_check_in = now;
            
            color = serenity::Color::from_rgb(46, 204, 113); // Green
            message = format!(
                "🔥 **Streak Extended!**\nGood job **{}**! Your daily streak is now at **{} days**.",
                user_name, profile.streak_count
            );
        // 3. Missed the day completely (Reset)
        } else {
            profile.streak_count = 1;
            profile.last_check_in = now;
            
            color = serenity::Color::from_rgb(231, 76, 60); // Red
            message = format!(
                "💔 **Streak Lost!**\nYou didn't check in yesterday. Your streak has reset to **1 day**."
            );
        }
    } else {
        // First time check-in setup
        let new_profile = UserProfile {
            username: user_name.clone(),
            streak_count: 1,
            last_check_in: now,
        };
        lock.insert(user_id.clone(), new_profile);
        
        color = serenity::Color::from_rgb(46, 204, 113); // Green
        message = format!(
            "🎉 **First Check-in!**\nWelcome **{}**! Your daily streak tracking has started at **1 day**.",
            user_name
        );
    }

    save_data(&lock);
    drop(lock);

    ctx.send(poise::CreateReply::default()
        .embed(serenity::CreateEmbed::new()
            .title("📆 Daily Streak System")
            .description(message)
            .color(color)
            .thumbnail(ctx.author().face())
            .timestamp(serenity::Timestamp::now())
        )
    ).await?;

    Ok(())
}

/// View the top server streaks leaderboard!
#[poise::command(slash_command, prefix_command)]
async fn leaderboard(ctx: Context<'_>) -> Result<(), Error> {
    let lock = ctx.data().profiles.lock().await;
    
    // Sort profiles by highest streak count
    let mut sorted_profiles: Vec<_> = lock.values().collect();
    sorted_profiles.sort_by(|a, b| b.streak_count.cmp(&a.streak_count));

    let mut leaderboard_text = String::new();
    
    if sorted_profiles.is_empty() {
        leaderboard_text = "_No active streaks yet. Be the first by using `/streak`!_".to_string();
    } else {
        // Take the top 10 users
        for (index, profile) in sorted_profiles.iter().take(10).enumerate() {
            let medal = match index {
                0 => "🥇".to_string(),
                1 => "🥈".to_string(),
                2 => "🥉".to_string(),
                _ => format!("{}.", index + 1),
            };
            
            leaderboard_text.push_str(&format!(
                "{} **{}** — `{} days` (Last active: <t:{}:R>)\n",
                medal,
                profile.username,
                profile.streak_count,
                profile.last_check_in.timestamp()
            ));
        }
    }

    ctx.send(poise::CreateReply::default()
        .embed(serenity::CreateEmbed::new()
            .title("🔥 Top Streak Leaderboard")
            .description(leaderboard_text)
            .color(serenity::Color::from_rgb(241, 196, 15)) // Gold Color
            .timestamp(serenity::Timestamp::now())
        )
    ).await?;

    Ok(())
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let token = std::env::var("DISCORD_TOKEN").expect("missing DISCORD_TOKEN");

    let initial_data = load_data();
    let profiles_mutex = Arc::new(Mutex::new(initial_data));

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![streak(), leaderboard()],
            prefix_options: poise::PrefixFrameworkOptions {
                prefix: Some("!".to_string()),
                ..Default::default()
            },
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(Data { profiles: profiles_mutex })
            })
        })
        .build();

    let client = serenity::ClientBuilder::new(token, serenity::GatewayIntents::non_privileged() | serenity::GatewayIntents::MESSAGE_CONTENT)
        .framework(framework)
        .await;

    println!("🚀 Streak Bot running. Use !streak, /streak, or /leaderboard!");
    client.unwrap().start().await.unwrap();
}

