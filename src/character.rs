use crate::models::{Skills, Item, MAX_VRXITEMS, MAX_WEAPONS, Weapon, TalentList, PrestigeList, BigI32Array, BigUpgradeArray};
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
                experience BIGINT,
                next_level BIGINT,
                administrator INT,
                level INT,
                speciality_points INT,
                weapon_points INT,
                respawn_weapon INT,
                frags INT,
                fragged INT,
                credits INT,
                weapon_respawns INT,
                class_num INT,
                boss INT,
                streak INT,
                current_health INT,
                max_health INT,
                current_armor INT,
                max_armor INT,
                shots BIGINT,
                shots_hit BIGINT,
                num_sprees INT,
                max_streak INT,
                suicides INT,
                teleports INT,
                spree_wars INT,
                break_sprees INT,
                break_spree_wars INT,
                num_2fers INT,
                flag_pickups INT,
                flag_captures INT,
                flag_returns INT,
                flag_kills INT,
                offense_kills INT,
                defense_kills INT,
                assists INT,
                playingtime INT,
                total_playtime INT,
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
                experience: row.get("experience"),
                next_level: row.get("next_level"),
                administrator: row.get("administrator"),
                level: row.get("level"),
                speciality_points: row.get("speciality_points"),
                weapon_points: row.get("weapon_points"),
                respawn_weapon: row.get("respawn_weapon"),
                frags: row.get::<i32, _>("frags") as u32,
                fragged: row.get::<i32, _>("fragged") as u32,
                credits: row.get::<i32, _>("credits") as u32,
                weapon_respawns: row.get::<i32, _>("weapon_respawns") as u32,
                class_num: row.get("class_num"),
                boss: row.get("boss"),
                streak: row.get("streak"),
                current_health: row.get("current_health"),
                max_health: row.get("max_health"),
                current_armor: row.get("current_armor"),
                max_armor: row.get("max_armor"),
                shots: row.get::<i64, _>("shots") as u64,
                shots_hit: row.get::<i64, _>("shots_hit") as u64,
                num_sprees: row.get::<i32, _>("num_sprees") as u32,
                max_streak: row.get("max_streak"),
                suicides: row.get("suicides"),
                teleports: row.get("teleports"),
                spree_wars: row.get("spree_wars"),
                break_sprees: row.get("break_sprees"),
                break_spree_wars: row.get("break_spree_wars"),
                num_2fers: row.get("num_2fers"),
                flag_pickups: row.get("flag_pickups"),
                flag_captures: row.get("flag_captures"),
                flag_returns: row.get("flag_returns"),
                flag_kills: row.get("flag_kills"),
                offense_kills: row.get("offense_kills"),
                defense_kills: row.get("defense_kills"),
                assists: row.get("assists"),
                playingtime: row.get("playingtime"),
                total_playtime: row.get("total_playtime"),
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
                name, experience, next_level, administrator, level, speciality_points, weapon_points, respawn_weapon,
                frags, fragged, credits, weapon_respawns, class_num, boss, streak, current_health, max_health,
                current_armor, max_armor, shots, shots_hit, num_sprees, max_streak, suicides, teleports,
                spree_wars, break_sprees, break_spree_wars, num_2fers, flag_pickups, flag_captures, flag_returns,
                flag_kills, offense_kills, defense_kills, assists, playingtime, total_playtime, inventory,
                password, member_since, last_played, owner, email, title, nerfme, items, weapons, abilities,
                talents, prestige
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20,
                $21, $22, $23, $24, $25, $26, $27, $28, $29, $30, $31, $32, $33, $34, $35, $36, $37, $38, $39,
                $40, $41, $42, $43, $44, $45, $46, $47, $48, $49, $50, $51)
            ON CONFLICT (name) DO UPDATE SET
                experience = EXCLUDED.experience, next_level = EXCLUDED.next_level, administrator = EXCLUDED.administrator,
                level = EXCLUDED.level, speciality_points = EXCLUDED.speciality_points, weapon_points = EXCLUDED.weapon_points,
                respawn_weapon = EXCLUDED.respawn_weapon, frags = EXCLUDED.frags, fragged = EXCLUDED.fragged,
                credits = EXCLUDED.credits, weapon_respawns = EXCLUDED.weapon_respawns, class_num = EXCLUDED.class_num,
                boss = EXCLUDED.boss, streak = EXCLUDED.streak, current_health = EXCLUDED.current_health,
                max_health = EXCLUDED.max_health, current_armor = EXCLUDED.current_armor, max_armor = EXCLUDED.max_armor,
                shots = EXCLUDED.shots, shots_hit = EXCLUDED.shots_hit, num_sprees = EXCLUDED.num_sprees,
                max_streak = EXCLUDED.max_streak, suicides = EXCLUDED.suicides, teleports = EXCLUDED.teleports,
                spree_wars = EXCLUDED.spree_wars, break_sprees = EXCLUDED.break_sprees, break_spree_wars = EXCLUDED.break_spree_wars,
                num_2fers = EXCLUDED.num_2fers, flag_pickups = EXCLUDED.flag_pickups, flag_captures = EXCLUDED.flag_captures,
                flag_returns = EXCLUDED.flag_returns, flag_kills = EXCLUDED.flag_kills, offense_kills = EXCLUDED.offense_kills,
                defense_kills = EXCLUDED.defense_kills, assists = EXCLUDED.assists, playingtime = EXCLUDED.playingtime,
                total_playtime = EXCLUDED.total_playtime, inventory = EXCLUDED.inventory, password = EXCLUDED.password,
                member_since = EXCLUDED.member_since, last_played = EXCLUDED.last_played, owner = EXCLUDED.owner,
                email = EXCLUDED.email, title = EXCLUDED.title, nerfme = EXCLUDED.nerfme, items = EXCLUDED.items,
                weapons = EXCLUDED.weapons, abilities = EXCLUDED.abilities, talents = EXCLUDED.talents,
                prestige = EXCLUDED.prestige
            "#
        )
        .bind(&skills.player_name)
        .bind(skills.experience)
        .bind(skills.next_level)
        .bind(skills.administrator)
        .bind(skills.level)
        .bind(skills.speciality_points)
        .bind(skills.weapon_points)
        .bind(skills.respawn_weapon)
        .bind(skills.frags as i32)
        .bind(skills.fragged as i32)
        .bind(skills.credits as i32)
        .bind(skills.weapon_respawns as i32)
        .bind(skills.class_num)
        .bind(skills.boss)
        .bind(skills.streak)
        .bind(skills.current_health)
        .bind(skills.max_health)
        .bind(skills.current_armor)
        .bind(skills.max_armor)
        .bind(skills.shots as i64)
        .bind(skills.shots_hit as i64)
        .bind(skills.num_sprees as i32)
        .bind(skills.max_streak)
        .bind(skills.suicides)
        .bind(skills.teleports)
        .bind(skills.spree_wars)
        .bind(skills.break_sprees)
        .bind(skills.break_spree_wars)
        .bind(skills.num_2fers)
        .bind(skills.flag_pickups)
        .bind(skills.flag_captures)
        .bind(skills.flag_returns)
        .bind(skills.flag_kills)
        .bind(skills.offense_kills)
        .bind(skills.defense_kills)
        .bind(skills.assists)
        .bind(skills.playingtime)
        .bind(skills.total_playtime)
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