use serde::{Deserialize, Serialize};
use serde_tuple::*;
use serde_big_array::BigArray;
use crate::messages::PlayerConnectionId;

pub const MAX_VRXITEMMODS: usize = 6;
pub const MAX_VRXITEMS: usize = 11;
pub const MAX_WEAPONS: usize = 13;
pub const MAX_WEAPONMODS: usize = 5;
pub const MAX_ABILITIES: usize = 160;
pub const MAX_TALENTS: usize = 15;
pub const MAX_ITEMS: usize = 256;

#[derive(Debug, Clone, Copy, Serialize_tuple, Deserialize_tuple)]
pub struct IModifier {
    pub modifier_type: i32,
    pub index: i32,
    pub value: i32,
    pub set: i32,
}

#[derive(Debug, Clone, Serialize_tuple, Deserialize_tuple)]
pub struct Item {
    pub item_type: i32,
    pub item_level: i32,
    pub quantity: i32,
    pub untradeable: i32,
    pub id: String,
    pub name: String,
    pub num_mods: i32,
    pub set_code: i32,
    pub class_num: i32,
    pub modifiers: [IModifier; MAX_VRXITEMMODS],
    pub is_unique: i32,
}

#[derive(Debug, Clone, Serialize_tuple, Deserialize_tuple)]
pub struct Upgrade {
    pub level: i32,
    pub current_level: i32,
    pub max_level: i32,
    pub hard_max: i32,
    pub modifier: i32,
    pub delay: f32,
    pub charge: i32,
    pub ammo: i32,
    pub max_ammo: i32,
    pub ammo_regenframe: i32,
    pub disable: i32,
    pub general_skill: i32,
    pub hidden: i32,
    pub runed: i32,
}

#[derive(Debug, Clone, Serialize_tuple, Deserialize_tuple)]
pub struct Talent {
    pub id: i32,
    pub upgrade_level: i32,
    pub max_level: i32,
    pub delay: f32,
}

#[derive(Debug, Clone, Serialize_tuple, Deserialize_tuple)]
pub struct TalentList {
    pub count: i32,
    pub talent_points: i32,
    pub talent: [Talent; MAX_TALENTS],
}

#[derive(Debug, Clone, Serialize_tuple, Deserialize_tuple)]
pub struct WeaponSkill {
    pub level: i32,
    pub current_level: i32,
    pub soft_max: i32,
    pub hard_max: i32,
}

#[derive(Debug, Clone, Serialize_tuple, Deserialize_tuple)]
pub struct Weapon {
    pub disable: i32,
    pub mods: [WeaponSkill; MAX_WEAPONMODS],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrestigeList {
    pub points: u32,
    pub total: u32,
    #[serde(with = "BigArray")]
    pub softmax_bump: [u8; MAX_ABILITIES],
    pub credit_level: u32,
    pub ability_points: u32,
    pub weapon_points: u32,
    pub class_skill: [u32; MAX_ABILITIES / 32 + 1],
}

#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub struct BigI32Array(#[serde(with = "BigArray")] pub [i32; MAX_ITEMS]);

impl std::ops::Deref for BigI32Array {
    type Target = [i32; MAX_ITEMS];
    fn deref(&self) -> &Self::Target { &self.0 }
}

impl std::clone::Clone for BigI32Array {
    fn clone(&self) -> Self { BigI32Array(self.0) }
}
impl std::fmt::Debug for BigI32Array {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { self.0.fmt(f) }
}

#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub struct BigUpgradeArray(#[serde(with = "BigArray")] pub [Upgrade; MAX_ABILITIES]);

impl std::ops::Deref for BigUpgradeArray {
    type Target = [Upgrade; MAX_ABILITIES];
    fn deref(&self) -> &Self::Target { &self.0 }
}

impl std::clone::Clone for BigUpgradeArray {
    fn clone(&self) -> Self { BigUpgradeArray(self.0.clone()) }
}

impl std::fmt::Debug for BigUpgradeArray {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { self.0.fmt(f) }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Progression {
    pub experience: i64,
    pub next_level: i64,
    pub administrator: i32,
    pub level: i32,
    pub speciality_points: i32,
    pub weapon_points: i32,
    pub respawn_weapon: i32,

    pub class_num: i32,
    pub boss: i32,

    pub current_health: i32,
    pub max_health: i32,
    pub current_armor: i32,
    pub max_armor: i32,
    pub credits: u32,
    pub weapon_respawns: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stats {
    pub max_streak: i32,
    pub frags: u32,
    pub fragged: u32,

    pub shots: u64,
    pub shots_hit: u64,

    pub num_sprees: u32,
    pub suicides: i32,
    pub teleports: i32,
    pub spree_wars: i32,
    pub break_sprees: i32,
    pub break_spree_wars: i32,
    pub num_2fers: i32,

    pub flag_pickups: i32,
    pub flag_captures: i32,
    pub flag_returns: i32,
    pub flag_kills: i32,
    pub offense_kills: i32,
    pub defense_kills: i32,
    pub assists: i32,

    pub playingtime: i32,
    pub total_playtime: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skills {
    pub progression: Progression,

    pub stats: Stats,

    pub inventory: BigI32Array,

    pub password: String,
    pub member_since: String,
    pub last_played: String,
    pub player_name: String,
    pub owner: String,
    pub email: String,
    pub title: String,

    pub nerfme: i32,
    pub connection_id: PlayerConnectionId,

    pub items: [Item; MAX_VRXITEMS],
    pub weapons: [Weapon; MAX_WEAPONS],
    pub abilities: BigUpgradeArray,

    pub talents: TalentList,
    pub prestige: PrestigeList,
}
