use crate::models::{Skills, Stats, Item, MAX_VRXITEMS, MAX_WEAPONS, Weapon, TalentList, PrestigeList, BigI32Array, BigUpgradeArray, Progression};
use sqlx::{PgPool, Row, types::Json};
use anyhow::Result;
use log::warn;

pub struct CharacterManager {
    pool: Option<PgPool>,
}

impl CharacterManager {
    pub fn new(pool: Option<PgPool>) -> Self {
        Self { pool }
    }

    pub async fn create_tables(&self) -> Result<()> {
        let pool = match &self.pool {
            Some(pool) => pool,
            None => {
                warn!("Database connection not available, skipping table creation");
                return Ok(());
            }
        };

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS characters (
                name TEXT PRIMARY KEY,
                progression JSONB,
                stats JSONB,
                inventory JSONB,
                password TEXT,
                member_since TEXT,
                last_played TEXT,
                owner TEXT,
                email TEXT,
                title TEXT,
                nerfme INT,
                items JSONB,
                weapons JSONB,
                abilities JSONB,
                talents JSONB,
                prestige JSONB
            );
            "#
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS stash (
                owner_name TEXT,
                page INT,
                slot INT,
                item JSONB,
                PRIMARY KEY (owner_name, page, slot)
            );
            "#
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn load_character(&self, name: &str) -> Result<Option<Skills>> {
        let pool = match &self.pool {
            Some(pool) => pool,
            None => return Ok(None),
        };

        let row = sqlx::query("SELECT * FROM characters WHERE name = $1")
            .bind(name)
            .fetch_optional(pool)
            .await?;

        if let Some(row) = row {
            Ok(Some(Skills {
                progression: row.get::<Json<Progression>, _>("progression").0,
                stats: row.get::<Json<Stats>, _>("stats").0,
                inventory: row.get::<Json<BigI32Array>, _>("inventory").0,
                password: row.get("password"),
                member_since: row.get("member_since"),
                last_played: row.get("last_played"),
                player_name: row.get("name"),
                owner: row.get("owner"),
                email: row.get("email"),
                title: row.get("title"),
                nerfme: row.get("nerfme"),
                connection_id: 0,
                items: row.get::<Json<[Item; MAX_VRXITEMS]>, _>("items").0,
                weapons: row.get::<Json<[Weapon; MAX_WEAPONS]>, _>("weapons").0,
                abilities: row.get::<Json<BigUpgradeArray>, _>("abilities").0,
                talents: row.get::<Json<TalentList>, _>("talents").0,
                prestige: row.get::<Json<PrestigeList>, _>("prestige").0,
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn save_character(&self, skills: &Skills) -> Result<()> {
        let pool = match &self.pool {
            Some(pool) => pool,
            None => return Ok(()),
        };

        sqlx::query(
            r#"
            INSERT INTO characters (
                name, progression, stats, inventory,
                password, member_since, last_played, owner, email, title, nerfme, items, weapons, abilities,
                talents, prestige
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
            ON CONFLICT (name) DO UPDATE SET
                progression = EXCLUDED.progression, stats = EXCLUDED.stats,
                inventory = EXCLUDED.inventory, password = EXCLUDED.password,
                member_since = EXCLUDED.member_since, last_played = EXCLUDED.last_played, owner = EXCLUDED.owner,
                email = EXCLUDED.email, title = EXCLUDED.title, nerfme = EXCLUDED.nerfme, items = EXCLUDED.items,
                weapons = EXCLUDED.weapons, abilities = EXCLUDED.abilities, talents = EXCLUDED.talents,
                prestige = EXCLUDED.prestige
            "#
        )
        .bind(&skills.player_name)
        .bind(Json(&skills.progression))
        .bind(Json(&skills.stats))
        .bind(Json(&skills.inventory))
        .bind(&skills.password)
        .bind(&skills.member_since)
        .bind(&skills.last_played)
        .bind(&skills.owner)
        .bind(&skills.email)
        .bind(&skills.title)
        .bind(skills.nerfme)
        .bind(Json(&skills.items))
        .bind(Json(&skills.weapons))
        .bind(Json(&skills.abilities))
        .bind(Json(&skills.talents))
        .bind(Json(&skills.prestige))
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_owner(&self, name: &str) -> Result<String> {
        let pool = match &self.pool {
            Some(pool) => pool,
            None => return Ok(name.to_string()),
        };

        let row = sqlx::query("SELECT owner, email FROM characters WHERE name = $1")
            .bind(name)
            .fetch_optional(pool)
            .await?;

        if let Some(row) = row {
            let owner: String = row.get("owner");
            let email: String = row.get("email");
            if !owner.is_empty() {
                Ok(owner)
            } else if !email.is_empty() {
                Ok(name.to_string())
            } else {
                Ok(name.to_string())
            }
        } else {
            Ok(name.to_string())
        }
    }

    pub async fn get_stash_page(&self, owner_name: &str, page: i32) -> Result<Vec<Option<Item>>> {
        let pool = match &self.pool {
            Some(pool) => pool,
            None => return Ok(vec![None; 64]),
        };

        let mut items = vec![None; 64];
        let rows = sqlx::query("SELECT slot, item FROM stash WHERE owner_name = $1 AND page = $2")
            .bind(owner_name)
            .bind(page)
            .fetch_all(pool)
            .await?;

        for row in rows {
            let slot: i32 = row.get("slot");
            let item: Json<Item> = row.get("item");
            if slot >= 0 && (slot as usize) < items.len() {
                items[slot as usize] = Some(item.0);
            }
        }
        Ok(items)
    }

    pub async fn put_stash_item(&self, owner_name: &str, page: i32, slot: i32, item: &Item) -> Result<bool> {
        let pool = match &self.pool {
            Some(pool) => pool,
            None => return Ok(false),
        };

        sqlx::query("INSERT INTO stash (owner_name, page, slot, item) VALUES ($1, $2, $3, $4) ON CONFLICT (owner_name, page, slot) DO UPDATE SET item = EXCLUDED.item")
            .bind(owner_name)
            .bind(page)
            .bind(slot)
            .bind(Json(item))
            .execute(pool)
            .await?;
        Ok(true)
    }

    pub async fn take_stash_item(&self, owner_name: &str, page: i32, slot: i32) -> Result<Option<Item>> {
        let pool = match &self.pool {
            Some(pool) => pool,
            None => return Ok(None),
        };

        let row = sqlx::query("DELETE FROM stash WHERE owner_name = $1 AND page = $2 AND slot = $3 RETURNING item")
            .bind(owner_name)
            .bind(page)
            .bind(slot)
            .fetch_optional(pool)
            .await?;

        if let Some(row) = row {
            let item: Json<Item> = row.get("item");
            Ok(Some(item.0))
        } else {
            Ok(None)
        }
    }

    pub async fn set_owner(&self, name: &str, password: &str, reset: bool, owner: &str) -> Result<()> {
        let pool = match &self.pool {
            Some(pool) => pool,
            None => return Ok(()),
        };

        if reset {
            sqlx::query("UPDATE characters SET owner = '', email = '' WHERE name = $1")
                .bind(name)
                .execute(pool)
                .await?;
        } else {
            sqlx::query("UPDATE characters SET owner = $1 WHERE name = $2 AND password = $3")
                .bind(owner)
                .bind(name)
                .bind(password)
                .execute(pool)
                .await?;
        }
        Ok(())
    }
}