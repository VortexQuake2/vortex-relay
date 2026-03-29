use crate::models::{Skills, Stats, Item, MAX_VRXITEMS, MAX_WEAPONS, Weapon, TalentList, PrestigeList, BigI32Array, BigUpgradeArray, Progression};
use sqlx::{PgPool, Row, types::Json};
use anyhow::Result;
use log::warn;
use sqlx::postgres::PgRow;
use crate::messages::{SetOwnerStatus, };

pub struct CharacterManager {
    pool: Option<PgPool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StashPosition {
    pub page: i32,
    pub slot: i32,
}

impl StashPosition {
    fn new(page: i32, slot: i32) -> Self {
        Self { page, slot }
    }

    pub fn find_free_slot(items: &Vec<StashPosition>) -> Result<StashPosition> {
        if items.is_empty() {
            Ok(StashPosition {
                page: 0,
                slot: 0
            })
        } else {
            let mut previous_slot = -1;
            let mut previous_page = -1;
            let free_slot;
            for row in items {

                // handle page changes
                // one or more page gap
                if row.page - previous_page > 1 {
                    // first slot of the page gap
                    break;
                }

                // no page gap, reset the slot counter
                if row.page - previous_page == 1 {
                    // on a page change, check if the slot is free
                    previous_slot = -1;
                    previous_page = row.page;
                }

                // handle slot changes
                // one or more slot gap
                if row.slot - previous_slot > 1 {
                    break;
                }

                // no slot gap
                if row.slot - previous_slot == 1 {
                    previous_slot = row.slot;
                }
            }

            if previous_page == -1 {
                // it's basically impossible for this to be -1 and for the previous slot to be > -1
                previous_page = 0;
            }

            if previous_page >= MAX_STASH_PAGES - 1 {
                anyhow::bail!("No free stash slot found");
            }

            if previous_slot < MAX_STASH_PAGE_ITEMS - 1 {
                free_slot = StashPosition { page: previous_page, slot: previous_slot + 1 };
            } else {
                free_slot = StashPosition { page: previous_page + 1, slot: 0 };
            }

            Ok(free_slot)
        }
    }
}

impl From<&PgRow> for StashPosition {

    fn from(value: &PgRow) -> Self {
        Self {
            page: value.get("page"),
            slot: value.get("slot"),
        }
    }
}

/// matches what's defined in C
const MAX_STASH_PAGE_ITEMS: i32 = 10;
const MAX_STASH_PAGES: i32 = 100;

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
                name TEXT PRIMARY KEY not null check (LENGTH(name) >= 3 and LENGTH(name) <= 16),
                progression JSONB NOT NULL,
                stats JSONB NOT NULL,
                inventory JSONB NOT NULL,
                password TEXT NOT NULL,
                member_since TEXT,
                last_played TEXT,
                owner TEXT null references characters(name) ON DELETE SET NULL,
                masterpassword TEXT null,
                title TEXT,
                nerfme INT,
                items JSONB NOT NULL,
                weapons JSONB NOT NULL,
                abilities JSONB not null ,
                talents JSONB not null,
                prestige JSONB not null,
                created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
            );
            "#
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS stash (
                owner_name TEXT references characters(name) ON DELETE CASCADE,
                page INT NOT NULL check (page >= 0 AND page < 100),
                slot INT NOT NULL check (slot >= 0 AND slot < 10),
                item JSONB NOT NULL,
                PRIMARY KEY (owner_name, page, slot)
            );
            "#
        )
        .execute(pool)
        .await?;

        // index on character name
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS characters_name_idx ON characters (name);
            "#
        ).execute(pool).await?;

        // index on stash owner name
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS stash_owner_name_idx ON stash (owner_name);
            "#
        ).execute(pool).await?;

        // index on stash page/slot
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS stash_page_slot_idx ON stash (page, slot);
            "#
        ).execute(pool).await?;

        // covering index on character owner from name
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS characters_owner_idx ON characters (owner);
            "#
        ).execute(pool).await?;

        Ok(())
    }

    pub async fn load_character(&self, name: &str) -> Result<Option<Box<Skills>>> {
        let pool = match &self.pool {
            Some(pool) => pool,
            None => return Ok(None),
        };

        let row = sqlx::query("SELECT * FROM characters WHERE name = $1")
            .bind(name)
            .fetch_optional(pool)
            .await?;

        if let Some(row) = row {
            Ok(Some(Box::from(Skills {
                progression: row.get::<Json<Progression>, _>("progression").0,
                stats: row.get::<Json<Stats>, _>("stats").0,
                inventory: row.get::<Json<BigI32Array>, _>("inventory").0,
                password: row.get("password"),
                member_since: row.get("member_since"),
                last_played: row.get("last_played"),
                player_name: row.get("name"),
                owner: row.get("owner"),
                master_password: row.get("masterpassword"),
                title: row.get("title"),
                nerfme: row.get("nerfme"),
                connection_id: 0,
                items: row.get::<Json<[Item; MAX_VRXITEMS]>, _>("items").0,
                weapons: row.get::<Json<[Weapon; MAX_WEAPONS]>, _>("weapons").0,
                abilities: row.get::<Json<BigUpgradeArray>, _>("abilities").0,
                talents: row.get::<Json<TalentList>, _>("talents").0,
                prestige: row.get::<Json<PrestigeList>, _>("prestige").0,
            })))
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
                password, member_since, last_played, owner, masterpassword, title, nerfme, items, weapons, abilities,
                talents, prestige
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
            ON CONFLICT (name) DO UPDATE SET
                progression = EXCLUDED.progression, stats = EXCLUDED.stats,
                inventory = EXCLUDED.inventory, password = EXCLUDED.password,
                member_since = EXCLUDED.member_since, last_played = EXCLUDED.last_played, owner = EXCLUDED.owner,
                masterpassword = EXCLUDED.masterpassword, title = EXCLUDED.title, nerfme = EXCLUDED.nerfme, items = EXCLUDED.items,
                weapons = EXCLUDED.weapons, abilities = EXCLUDED.abilities, talents = EXCLUDED.talents,
                prestige = EXCLUDED.prestige,
                updated_at = CURRENT_TIMESTAMP
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
        .bind(&skills.master_password)
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

        let row = sqlx::query("SELECT owner FROM characters WHERE name = $1")
            .bind(name)
            .fetch_optional(pool)
            .await?;

        if let Some(row) = row {
            if let Ok(owner) = row.try_get("owner") {
                Ok(owner)
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

    pub async fn find_free_stash_slot(&self, owner_name: &str) -> Result<StashPosition> {
        let pool = match &self.pool {
            Some(pool) => pool,
            None => anyhow::bail!("Database connection not available")
        };

        let row = sqlx::query(r#"
        SELECT page, slot FROM stash WHERE
        owner_name = $1 AND slot >= 0
        ORDER BY page, slot
    "#)
            .bind(owner_name)
            .fetch_all(pool)
            .await?;

        let row = row.iter()
            .map(|r| StashPosition::from(r))
            .collect::<Vec<_>>();

        StashPosition::find_free_slot(&row)
    }


    pub async fn put_stash_item(&self, owner_name: &str, pos: StashPosition, item: &Item) -> Result<bool> {
        let pool = match &self.pool {
            Some(pool) => pool,
            None => return Ok(false),
        };

        sqlx::query(r#"
        INSERT INTO stash (owner_name, page, slot, item)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (owner_name, page, slot) DO UPDATE SET item = EXCLUDED.item
         "#)
            .bind(owner_name)
            .bind(pos.page)
            .bind(pos.slot)
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

    pub async fn set_owner(&self, name: &str, password: &str, reset: bool, owner: &str) -> Result<SetOwnerStatus> {
        let pool = match &self.pool {
            Some(pool) => pool,
            None => return Ok(SetOwnerStatus::InternalError),
        };

        if reset {
            sqlx::query("UPDATE characters SET owner = '' WHERE name = $1")
                .bind(name)
                .execute(pool)
                .await?;
        } else {
            let has_owner = sqlx::query(r#"
                SELECT (select owner from characters where name = $1)          as owner,
                       (select masterpassword from characters where name = $2) as masterpassword
                "#)
                .bind(name)
                .bind(owner)
                .fetch_optional(pool)
                .await?;

            if let Some(row) = has_owner {
                if let Ok(owner) = row.try_get::<String, &str>("owner") {
                    if owner != "" {
                        return Ok(SetOwnerStatus::OwnerAlreadySet);
                    }
                }

                if let Ok(_password) = row.try_get::<String, &str>("masterpassword") {
                    if _password != password {
                        return Ok(SetOwnerStatus::WrongPassword);
                    }
                }
            } else {
                return Ok(SetOwnerStatus::CharacterNotFound);
            }

            let e = sqlx::query(r#"
            UPDATE characters SET owner = $1 WHERE
            name = $2
            AND (select masterpassword from characters where name = $1) = $3
            "#)
                .bind(owner)
                .bind(name)
                .bind(password)
                .execute(pool)
                .await?;

            // this should not happen since we did the checks
            if e.rows_affected() == 0 {
                return Ok(SetOwnerStatus::InternalError);
            }
        }

        Ok(SetOwnerStatus::Ok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_free_slot() {
        let items = vec![
            StashPosition::new(0, 0),
            StashPosition::new(0, 1),
        ];

        assert_eq!(StashPosition::find_free_slot(&items).unwrap(), StashPosition::new(0, 2));

        let items = vec![
            StashPosition::new(0, 1),
        ];

        assert_eq!(StashPosition::find_free_slot(&items).unwrap(), StashPosition::new(0, 0));

        let items = vec![
            StashPosition::new(1, 0),
        ];
        assert_eq!(StashPosition::find_free_slot(&items).unwrap(), StashPosition::new(0, 0));

        let mut items = vec![];
        for i in 0..10 {
            items.push(StashPosition::new(0, i));
        }

        assert_eq!(StashPosition::find_free_slot(&items).unwrap(), StashPosition::new(1, 0));

        let mut items = vec![];
        for i in 0..3 {
            for j in 0..10 {
                items.push(StashPosition::new(i, j));
            }
        }

        assert_eq!(StashPosition::find_free_slot(&items).unwrap(), StashPosition::new(3, 0));

        let items = vec![
            StashPosition::new(0, 0),
            StashPosition::new(0, 1),
            StashPosition::new(0, 2),
            StashPosition::new(0, 4),
        ];

        assert_eq!(StashPosition::find_free_slot(&items).unwrap(), StashPosition::new(0, 3));

        let mut items = vec![];
        for i in 0..5 {
            for j in 0..10 {
                if i == 3 && j == 4 {
                    continue
                }
                items.push(StashPosition::new(i, j));
            }
        }

        assert_eq!(StashPosition::find_free_slot(&items).unwrap(), StashPosition::new(3, 4));

        let items = vec![];
        assert_eq!(StashPosition::find_free_slot(&items).unwrap(), StashPosition::new(0, 0));
    }
}