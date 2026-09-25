use core::sync::atomic::{AtomicUsize, AtomicU64, Ordering};

use crate::buf::Buf;
use crate::log;
use crate::mem::*;

// Offsets::GWorld
static GWORLD_SLOT: AtomicUsize = AtomicUsize::new(0);
static NEXT_GWORLD_SCAN: AtomicU64 = AtomicU64::new(0);
// UWorld.OwningGameInstance
const OFF_WORLD_GI: usize = 0x230;
// UGameInstance.LocalPlayers
const OFF_GI_PLAYERS: usize = 0x38;
// UPlayer.PlayerController
const OFF_PLAYER_PC: usize = 0x30;
// APlayerController.PlayerCameraManager
const OFF_PC_MANAGER: usize = 0x380;
// FCameraSettings::PlayerFadeDistance::{bAmend u8@0, TargetValue f32@4}
const OFF_FADE_AMEND: usize = 0x558;
const OFF_FADE_TARGET: usize = 0x55C;
// FCameraSettings::PlayerHideDistance::{bAmend u8@0, TargetValue f32@4}
const OFF_HIDE_AMEND: usize = 0x560;
const OFF_HIDE_TARGET: usize = 0x564;
// AHTPlayerCameraManager members with an FCameraSettings base.
const SETTINGS: [usize; 28] = [
    0x4940, // RunSettings
    0x5028, // FightingSettings
    0x5700, // InLiftSettings
    0x5C70, // JumpingSettings
    0x61E8, // LockTargetSettings
    0x6768, // SpecialBoxSettings
    0x6CE0, // TimeClockSettings
    0x7258, // BossCameraSettings
    0x77D0, // TenacityBreakCameraSettings
    0x7DD8, // SwimmingSettings
    0x8350, // ClimbingSettings
    0x8910, // ShopSettings
    0x8ED0, // CarTPSettings
    0x9460, // SitSettings
    0x99D0, // BoardTrainSettings
    0x9F60, // DialogueCameraSettings
    0xA4F8, // LoadingCameraSettings
    0xAA78, // GlidingCameraSettings
    0xAFE8, // LiningTargetCameraSettings
    0xB558, // RollerCoasterCameraSettings
    0xBAC8, // NormalCameraSettings
    0xC038, // RideNormalCameraSettings
    0xC5A8, // LockNpcSettings
    0xCBD0, // SelfieSettings
    0xD280, // CustomCameraSetting
    0xD840, // VersatileSettings
    0xDDB0, // LastStateRealVersatileSettings
    0xEB90, // SelfCameraSetting
];
// AHTPlayerCameraManager::ActorFadeCurve
const ACTOR_FADE_CURVE: usize = 0x37C8;
// AHTPlayerCameraManager::CollisionFadeTotalDuration
const COLLISION_FADE_DURATION: usize = 0x3820;
// AHTPlayerCameraManager::PlayerFadeSpeed
const PLAYER_FADE_SPEED: usize = 0x3B50;
// AHTPlayerCameraManager::PlayerFadeDistanceSquare
const PLAYER_FADE_DISTANCE_SQUARE: usize = 0x3B54;
// AHTPlayerCameraManager::PlayerHideDistanceSquare
const PLAYER_HIDE_DISTANCE_SQUARE: usize = 0x3B58;
// AHTPlayerCameraManager::PlayerPitchFadeCurve
const PLAYER_PITCH_FADE_CURVE: usize = 0x3B60;

const OFF_TARRAY_DATA: usize = 0x00;
const OFF_TARRAY_NUM: usize = 0x08;
const SCAN_CHUNK: usize = 0x1000;
const SCAN_RETRY_ROUNDS: u64 = 2;
const EXPECT_SPEED: f32 = 2.;
const EXPECT_FADE_DIST: f32 = 10000.;
const EXPECT_HIDE_DIST: f32 = 6400.;
const RF_CDO_OR_ARCHETYPE: u32 = 0x10 | 0x20;

static TARGET: AtomicUsize = AtomicUsize::new(0);
static CUR_MANAGER: AtomicUsize = AtomicUsize::new(0);
static PATCHED_TOTAL: AtomicU64 = AtomicU64::new(0);
static REPATCHED: AtomicU64 = AtomicU64::new(0);
static LAST_REPATCH_ROUND: AtomicU64 = AtomicU64::new(0);
static ROUNDS: AtomicU64 = AtomicU64::new(0);
// re-patch lines are throttled
const REPATCH_LOG_EVERY_ROUNDS: u64 = 120;

// intact() reason codes: which part broke
const INTACT_OK: u8 = 0;
const BROKEN_AMEND: u8 = 1; // override flag was reset
const BROKEN_TARGET: u8 = 2; // flag held but target value changed
const BROKEN_CACHED: u8 = 3; // curves / cached distances restored

pub fn rounds() -> u64 {
    ROUNDS.load(Ordering::Relaxed)
}

pub fn patched_total() -> u64 {
    PATCHED_TOTAL.load(Ordering::Relaxed)
}

pub fn manager() -> usize {
    CUR_MANAGER.load(Ordering::Relaxed)
}

pub fn repatched() -> u64 {
    REPATCHED.load(Ordering::Relaxed)
}

/// Resolve the current camera manager through the controller chain. None while loading / between worlds.
fn resolve(module: &Module) -> Option<usize> {
    let slot = GWORLD_SLOT.load(Ordering::Acquire);
    if slot == 0 || !module.contains(slot) {
        return None;
    }
    let world = read_u64(slot)? as usize;
    resolve_world(module, world)
}

fn resolve_world(module: &Module, world: usize) -> Option<usize> {
    if world == 0 || !valid_object(module, world) || is_cdo_or_archetype(world) {
        return None;
    }
    let gi = read_u64(world + OFF_WORLD_GI).unwrap_or(0) as usize;
    if gi == 0 || !valid_object(module, gi) {
        return None;
    }
    let players = gi + OFF_GI_PLAYERS;
    let data = read_u64(players + OFF_TARRAY_DATA).unwrap_or(0) as usize;
    let num = read_u32(players + OFF_TARRAY_NUM).unwrap_or(0);
    if data == 0 || num == 0 {
        return None;
    }
    let lp = read_u64(data).unwrap_or(0) as usize;
    if lp == 0 || !valid_object(module, lp) {
        return None;
    }
    let pc = read_u64(lp + OFF_PLAYER_PC).unwrap_or(0) as usize;
    if pc == 0 || !valid_object(module, pc) {
        return None;
    }
    let mgr = read_u64(pc + OFF_PC_MANAGER).unwrap_or(0) as usize;
    if mgr == 0 || !valid_object(module, mgr) {
        return None;
    }
    Some(mgr)
}

fn valid_object(module: &Module, obj: usize) -> bool {
    match read_u64(obj) {
        Some(v) => v != 0 && module.contains(v as usize),
        None => false,
    }
}

fn is_cdo_or_archetype(obj: usize) -> bool {
    matches!(read_u32(obj + 0x08), Some(f) if f & RF_CDO_OR_ARCHETYPE != 0)
}

fn shipped_values(obj: usize) -> bool {
    is_default(
        read_f32(obj + PLAYER_FADE_SPEED).unwrap_or(f32::NAN),
        read_f32(obj + PLAYER_FADE_DISTANCE_SQUARE).unwrap_or(f32::NAN),
        read_f32(obj + PLAYER_HIDE_DISTANCE_SQUARE).unwrap_or(f32::NAN),
    )
}

fn is_default(speed: f32, fade_dist: f32, hide_dist: f32) -> bool {
    (speed - EXPECT_SPEED).abs() < 1e-4
        && (fade_dist - EXPECT_FADE_DIST).abs() < 1e-4
        && (hide_dist - EXPECT_HIDE_DIST).abs() < 1e-4
}

/// reason the manager is not intact (INTACT_OK when everything holds)
fn intact_reason(mgr: usize) -> u8 {
    let mut i = 0usize;
    while i < SETTINGS.len() {
        let base = mgr + SETTINGS[i];
        if read_u8(base + OFF_FADE_AMEND) != Some(1)
            || read_u8(base + OFF_HIDE_AMEND) != Some(1)
        {
            return BROKEN_AMEND;
        }
        if read_f32(base + OFF_FADE_TARGET) != Some(0.0)
            || read_f32(base + OFF_HIDE_TARGET) != Some(0.0)
        {
            return BROKEN_TARGET;
        }
        i += 1;
    }
    if read_u64(mgr + ACTOR_FADE_CURVE) == Some(0)
        && read_u64(mgr + PLAYER_PITCH_FADE_CURVE) == Some(0)
        && read_f32(mgr + COLLISION_FADE_DURATION) == Some(0.0)
        && read_f32(mgr + PLAYER_FADE_DISTANCE_SQUARE) == Some(0.0)
        && read_f32(mgr + PLAYER_HIDE_DISTANCE_SQUARE) == Some(0.0)
    {
        INTACT_OK
    } else {
        BROKEN_CACHED
    }
}

/// force the override flag + zero target on all source structs, then clear the cached fields
fn patch(mgr: usize) -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < SETTINGS.len() {
        let base = mgr + SETTINGS[i];
        ok &= write_u8(base + OFF_FADE_AMEND, 1);
        ok &= write_f32(base + OFF_FADE_TARGET, 0.0);
        ok &= write_u8(base + OFF_HIDE_AMEND, 1);
        ok &= write_f32(base + OFF_HIDE_TARGET, 0.0);
        i += 1;
    }
    ok &= write_u64(mgr + ACTOR_FADE_CURVE, 0);
    ok &= write_u64(mgr + PLAYER_PITCH_FADE_CURVE, 0);
    ok &= write_f32(mgr + COLLISION_FADE_DURATION, 0.0);
    ok &= write_f32(mgr + PLAYER_FADE_DISTANCE_SQUARE, 0.0);
    ok &= write_f32(mgr + PLAYER_HIDE_DISTANCE_SQUARE, 0.0);
    ok
}

pub fn init(module: &Module) {
    if discover_gworld(module) {
        return;
    }
    NEXT_GWORLD_SCAN.store(SCAN_RETRY_ROUNDS, Ordering::Relaxed);
    log::line("GWorld slot not found; scan will retry");
}

fn discover_gworld(module: &Module) -> bool {
    let mut buf = [0u8; SCAN_CHUNK];
    let end = module.end();
    let mut addr = module.base;
    while addr < end {
        let remaining = end - addr;
        let want = if remaining < SCAN_CHUNK { remaining } else { SCAN_CHUNK };
        let got = unsafe { read_raw(addr, buf.as_mut_ptr(), want) };
        let usable = got & !7usize;
        let mut i = 0usize;
        while i < usable {
            let world = u64::from_ne_bytes([
                buf[i],
                buf[i + 1],
                buf[i + 2],
                buf[i + 3],
                buf[i + 4],
                buf[i + 5],
                buf[i + 6],
                buf[i + 7],
            ]) as usize;
            let slot = addr + i;
            if world != 0
                && (world & 7) == 0
                && !module.contains(world)
                && resolve_world(module, world).is_some()
            {
                GWORLD_SLOT.store(slot, Ordering::Release);
                let mut b = Buf::new();
                b.push_str("GWorld slot 0x");
                b.push_hex(slot as u64, 0);
                b.push_str(" (RVA 0x");
                b.push_hex(slot.wrapping_sub(module.base) as u64, 0);
                b.push_str("), world 0x");
                b.push_hex(world as u64, 0);
                log::log_buf(&b);
                return true;
            }
            i += 8;
        }
        if want == 0 {
            break;
        }
        addr += want;
    }
    false
}

pub fn tick(module: &Module) {
    let round = ROUNDS.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    if GWORLD_SLOT.load(Ordering::Acquire) == 0 {
        if round >= NEXT_GWORLD_SCAN.load(Ordering::Relaxed) {
            NEXT_GWORLD_SCAN.store(
                round.wrapping_add(SCAN_RETRY_ROUNDS),
                Ordering::Relaxed,
            );
            discover_gworld(module);
        }
        return;
    }
    let Some(mgr) = resolve(module) else {
        return;
    };
    if is_cdo_or_archetype(mgr) {
        return;
    }

    let vtable = match read_u64(mgr) {
        Some(v) => v as usize,
        None => return,
    };
    let known = TARGET.load(Ordering::Relaxed);
    if known == 0 {
        // first contact: only trust the shipped fingerprint, then lock on
        if !shipped_values(mgr) {
            return;
        }
        TARGET.store(vtable, Ordering::Relaxed);
        log_learned(module, vtable);
    } else if vtable != known {
        // a manager class we have never seen
        if !shipped_values(mgr) {
            return;
        }
        TARGET.store(vtable, Ordering::Relaxed);
        log_learned(module, vtable);
    }

    let new_mgr = CUR_MANAGER.swap(mgr, Ordering::Relaxed) != mgr;
    if new_mgr {
        let mut b = Buf::new();
        b.push_str("manager 0x");
        b.push_hex(mgr as u64, 0);
        log::log_buf(&b);
    }

    let reason = intact_reason(mgr);
    if reason == INTACT_OK {
        return;
    }
    if !patch(mgr) {
        log::line("  patch write FAILED");
        return;
    }
    PATCHED_TOTAL.fetch_add(1, Ordering::Relaxed);
    if new_mgr {
        log_patched(mgr);
        return;
    }
    // same manager broken again
    REPATCHED.fetch_add(1, Ordering::Relaxed);
    let round = ROUNDS.load(Ordering::Relaxed);
    if round.wrapping_sub(LAST_REPATCH_ROUND.load(Ordering::Relaxed))
        >= REPATCH_LOG_EVERY_ROUNDS
    {
        LAST_REPATCH_ROUND.store(round, Ordering::Relaxed);
        let mut b = Buf::new();
        b.push_str("  re-patched 0x");
        b.push_hex(mgr as u64, 0);
        b.push_str(if reason == BROKEN_AMEND {
            " (amend reset)"
        } else if reason == BROKEN_TARGET {
            " (target restored)"
        } else {
            " (cached restored)"
        });
        log::log_buf(&b);
    }
}

fn log_learned(module: &Module, vtable: usize) {
    let mut b = Buf::new();
    b.push_str("target class learned: vtable 0x");
    b.push_hex(vtable as u64, 0);
    b.push_str(" (RVA 0x");
    b.push_hex(vtable.wrapping_sub(module.base) as u64, 0);
    b.push_str(")");
    log::log_buf(&b);
}

fn log_patched(mgr: usize) {
    let mut b = Buf::new();
    b.push_str("  patched manager 0x");
    b.push_hex(mgr as u64, 0);
    log::log_buf(&b);
}
