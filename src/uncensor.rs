use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use crate::xx::buf::Buf;
use crate::xx::ffi;
use crate::xx::log;
use crate::xx::mem::*;

// From the dumper-7 CppSDK dump (CppSDK/SDK/Engine_classes.hpp).
// UWorld::OwningGameInstance
const OFF_WORLD_GI: usize = 0x230;
// UGameInstance::LocalPlayers
const OFF_GI_PLAYERS: usize = 0x38;
// UPlayer::PlayerController
const OFF_PLAYER_PC: usize = 0x30;
// APlayerController::PlayerCameraManager
const OFF_PC_MANAGER: usize = 0x380;
// TArray<UPlayer*>: Data @ +0x00, Num @ +0x08
const OFF_TARRAY_NUM: usize = 0x08;

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

// object header block
const OBJ_BLOCK: usize = 0x28;
// UE object header: VTable(0x00) ObjectFlags(0x08) InternalIndex(0x0C) Class(0x10) Name(0x18)
const OFF_FLAGS: usize = 0x08;
const OFF_CLASS: usize = 0x10;
// RF_ClassDefaultObject(0x10) | RF_ArchetypeObject(0x20): never live instances
const RF_CDO_OR_ARCHETYPE: u32 = 0x10 | 0x20;
const RF_DESTROYED: u32 = 0x8000 | 0x10000;
const SCAN_CHUNK: usize = 0x10000;

const EXPECT_SPEED: f32 = 2.;
const EXPECT_FADE_DIST: f32 = 10000.;
const EXPECT_HIDE_DIST: f32 = 6400.;
// Re-patch lines are throttled to one every five minutes.
const REPATCH_LOG_INTERVAL_MS: u64 = 300_000;
// intact_reason() reason codes: which part broke
const INTACT_OK: u8 = 0;
// override flag was reset
const BROKEN_AMEND: u8 = 1;
// flag held but target value changed
const BROKEN_TARGET: u8 = 2;
// curves / cached distances restored
const BROKEN_CACHED: u8 = 3;

static WORLD_SLOT: AtomicUsize = AtomicUsize::new(0);
static RESOLVE_TRIES: AtomicU64 = AtomicU64::new(0);
static NEXT_SCAN: AtomicU64 = AtomicU64::new(0);

static CUR_MANAGER: AtomicUsize = AtomicUsize::new(0);
static TARGET_VTABLE: AtomicUsize = AtomicUsize::new(0);
static PATCHES: AtomicU64 = AtomicU64::new(0);
static REPATCHES: AtomicU64 = AtomicU64::new(0);
static LAST_REPATCH_MS: AtomicU64 = AtomicU64::new(0);

struct Described {
    vtable: usize,
    flags: u32,
}

/// Reads the object header once; rejects dead objects.
/// Template checks remain caller-specific because measure also uses CDOs.
fn describe(module: &Module, obj: usize) -> Option<Described> {
    if obj == 0 || obj % 8 != 0 {
        return None;
    }
    let mut blk = [0u8; OBJ_BLOCK];
    if unsafe { read_raw(obj, blk.as_mut_ptr(), blk.len()) } != blk.len() {
        return None;
    }
    let vtable = rd_u64(&blk, 0) as usize;
    let flags = rd_u32(&blk, OFF_FLAGS);
    let class = rd_u64(&blk, OFF_CLASS) as usize;
    if !module.contains(vtable)
        || flags & RF_DESTROYED != 0
        || class == 0
        || class % 8 != 0
    {
        return None;
    }
    Some(Described { vtable, flags })
}

fn valid_object(module: &Module, obj: usize) -> bool {
    describe(module, obj).is_some()
}

#[cfg(feature = "measure")]
fn is_cdo_or_archetype(obj: usize) -> bool {
    matches!(read_u32(obj + OFF_FLAGS), Some(f) if f & RF_CDO_OR_ARCHETYPE != 0)
}

fn array(addr: usize, limit: u32) -> Option<(usize, u32)> {
    let data = read_u64(addr)? as usize;
    let num = read_u32(addr + OFF_TARRAY_NUM)?;
    if data == 0 || num == 0 || num > limit {
        return None;
    }
    Some((data, num))
}

fn rd_u32(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}

fn rd_u64(b: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(b[i..i + 8].try_into().unwrap())
}

fn rd_f32(b: &[u8], i: usize) -> f32 {
    f32::from_bits(rd_u32(b, i))
}

fn scan<F>(module: &Module, window: usize, mut test: F) -> Option<usize>
where
    F: FnMut(usize, &[u8], usize) -> bool,
{
    let mut sections = [Section { start: 0, len: 0 }; MAX_SECTIONS];
    let n = module.writable_sections(&mut sections);
    let mut buf = [0u8; SCAN_CHUNK];
    let overlap = window - 8;
    let mut s = 0usize;
    while s < n {
        let start = sections[s].start;
        let len = sections[s].len;
        let mut off = 0usize;
        while off < len {
            let left = len - off;
            let want = if left < SCAN_CHUNK { left } else { SCAN_CHUNK };
            let got = unsafe { read_raw(start + off, buf.as_mut_ptr(), want) };
            let usable = if got < window { 0 } else { got & !7 };
            let mut i = 0usize;
            while i + window <= usable {
                if test(start + off + i, &buf, i) {
                    return Some(start + off + i);
                }
                i += 8;
            }
            let step = if got == want && got > window {
                got - overlap
            } else if usable > window {
                usable - overlap
            } else {
                want
            };
            off += step;
        }
        s += 1;
    }
    None
}

/// Walks World -> GameInstance -> LocalPlayers -> PlayerController -> PlayerCameraManager.
/// Returns None when any required link is missing or invalid.
fn resolve_world(module: &Module, world: usize) -> Option<(usize, usize)> {
    let d = describe(module, world)?;
    if d.flags & RF_CDO_OR_ARCHETYPE != 0 {
        return None;
    }
    let gi = read_u64(world + OFF_WORLD_GI).unwrap_or(0) as usize;
    if gi == 0 || !valid_object(module, gi) {
        return None;
    }
    let (data, _num) = array(gi + OFF_GI_PLAYERS, 32)?;
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
    Some((pc, mgr))
}

fn discover_gworld(module: &Module) -> Option<usize> {
    scan(module, 8, |_at, b, i| {
        let world = rd_u64(b, i) as usize;
        world != 0
            && (world & 7) == 0
            && !module.contains(world)
            && resolve_world(module, world).is_some()
    })
}

/// Finds the GWorld slot, retrying a few ticks apart.
#[cfg(not(feature = "measure"))]
fn ensure_globals(module: &Module) -> bool {
    if WORLD_SLOT.load(Ordering::Relaxed) != 0 {
        return true;
    }
    let tries = RESOLVE_TRIES.fetch_add(1, Ordering::Relaxed);
    if tries < NEXT_SCAN.load(Ordering::Relaxed) {
        return false;
    }
    // wait 4 ticks for next gworld scan
    NEXT_SCAN.store(tries + 4, Ordering::Relaxed);

    if WORLD_SLOT.load(Ordering::Relaxed) == 0 {
        match discover_gworld(module) {
            Some(slot) => {
                WORLD_SLOT.store(slot, Ordering::Relaxed);
                let world = read_u64(slot).unwrap_or(0) as usize;
                log_gworld(slot, world, module);
            }
            None => {
                if tries == 0 {
                    log::line("GWorld not found yet; the scan will retry");
                }
                return false;
            }
        }
    }

    let world = read_u64(WORLD_SLOT.load(Ordering::Relaxed)).unwrap_or(0) as usize;
    world != 0
}

fn log_gworld(slot: usize, world: usize, module: &Module) {
    let mut b = Buf::new();
    b.push_str("GWorld slot 0x");
    b.push_hex(slot as u64, 0);
    b.push_str(" (RVA 0x");
    b.push_hex(slot.wrapping_sub(module.base) as u64, 0);
    b.push_str("), world 0x");
    b.push_hex(world as u64, 0);
    log::log_buf(&b);
}

fn shipped_values(obj: usize) -> bool {
    // The shipped fingerprint is three adjacent floats; read them together.
    let mut blk = [0u8; PLAYER_HIDE_DISTANCE_SQUARE + 4 - PLAYER_FADE_SPEED];
    if unsafe { read_raw(obj + PLAYER_FADE_SPEED, blk.as_mut_ptr(), blk.len()) } != blk.len() {
        return false;
    }
    let speed = rd_f32(&blk, 0);
    let fade_dist = rd_f32(&blk, PLAYER_FADE_DISTANCE_SQUARE - PLAYER_FADE_SPEED);
    let hide_dist = rd_f32(&blk, PLAYER_HIDE_DISTANCE_SQUARE - PLAYER_FADE_SPEED);
    (speed - EXPECT_SPEED).abs() < 1e-4
        && (fade_dist - EXPECT_FADE_DIST).abs() < 1e-4
        && (hide_dist - EXPECT_HIDE_DIST).abs() < 1e-4
}

/// Returns why the manager is not intact (INTACT_OK when everything holds).
fn intact_reason(mgr: usize) -> u8 {
    let mut i = 0usize;
    while i < SETTINGS.len() {
        let base = mgr + SETTINGS[i];
        // The two FCameraInstantSetting structs occupy one 16-byte span.
        let mut blk = [0u8; OFF_HIDE_TARGET + 4 - OFF_FADE_AMEND];
        if unsafe { read_raw(base + OFF_FADE_AMEND, blk.as_mut_ptr(), blk.len()) } != blk.len()
            || blk[0] != 1
            || blk[OFF_HIDE_AMEND - OFF_FADE_AMEND] != 1
        {
            return BROKEN_AMEND;
        }
        if rd_f32(&blk, OFF_FADE_TARGET - OFF_FADE_AMEND) != 0.0
            || rd_f32(&blk, OFF_HIDE_TARGET - OFF_FADE_AMEND) != 0.0
        {
            return BROKEN_TARGET;
        }
        i += 1;
    }
    let mut blk = [0u8; PLAYER_PITCH_FADE_CURVE + 8 - PLAYER_FADE_DISTANCE_SQUARE];
    if unsafe { read_raw(mgr + PLAYER_FADE_DISTANCE_SQUARE, blk.as_mut_ptr(), blk.len()) } != blk.len() {
        return BROKEN_CACHED;
    }
    if rd_f32(&blk, 0) == 0.0
        && rd_f32(&blk, PLAYER_HIDE_DISTANCE_SQUARE - PLAYER_FADE_DISTANCE_SQUARE) == 0.0
        && rd_u64(&blk, PLAYER_PITCH_FADE_CURVE - PLAYER_FADE_DISTANCE_SQUARE) == 0
        && read_u64(mgr + ACTOR_FADE_CURVE) == Some(0)
        && read_f32(mgr + COLLISION_FADE_DURATION) == Some(0.0)
    {
        INTACT_OK
    } else {
        BROKEN_CACHED
    }
}

/// Forces the override flag + zero target on all source structs, then clears cached fields.
fn patch(module: &Module, mgr: usize, vtable: usize) -> bool {
    // Recheck immediately before writing: a manager can disappear between reads.
    let Some(d) = describe(module, mgr) else {
        return false;
    };
    if d.vtable != vtable || d.flags & RF_CDO_OR_ARCHETYPE != 0 {
        return false;
    }
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

pub fn tick(module: &Module) {
    if !ensure_globals(module) {
        CUR_MANAGER.store(0, Ordering::Relaxed);
        return;
    }
    let world = read_u64(WORLD_SLOT.load(Ordering::Relaxed)).unwrap_or(0) as usize;
    let Some((_controller, mgr)) = resolve_world(module, world) else {
        CUR_MANAGER.store(0, Ordering::Relaxed);
        return;
    };
    let Some(d) = describe(module, mgr) else {
        CUR_MANAGER.store(0, Ordering::Relaxed);
        return;
    };
    if d.flags & RF_CDO_OR_ARCHETYPE != 0 {
        CUR_MANAGER.store(0, Ordering::Relaxed);
        return;
    }

    if d.vtable != TARGET_VTABLE.load(Ordering::Relaxed) {
        // First contact with this vtable: only trust the shipped fingerprint.
        if !shipped_values(mgr) {
            CUR_MANAGER.store(0, Ordering::Relaxed);
            return;
        }
        TARGET_VTABLE.store(d.vtable, Ordering::Relaxed);
        log_learned(module, d.vtable);
    }

    let new_mgr = CUR_MANAGER.swap(mgr, Ordering::Relaxed) != mgr;
    if new_mgr {
        log_object("manager", mgr);
    }

    // Scene and dialogue events can restore settings without replacing the manager.
    // Recheck every tick, but only write when the override or cached values changed.
    let reason = intact_reason(mgr);
    if reason == INTACT_OK {
        return;
    }
    if !patch(module, mgr, d.vtable) {
        log::line("patch write FAILED");
        return;
    }
    PATCHES.fetch_add(1, Ordering::Relaxed);
    let now = unsafe { ffi::GetTickCount64() };
    if new_mgr {
        LAST_REPATCH_MS.store(now, Ordering::Relaxed);
        log_object("patched manager", mgr);
        return;
    }
    // Same manager broken again; keep the total even when the detail log is throttled.
    REPATCHES.fetch_add(1, Ordering::Relaxed);
    if now.wrapping_sub(LAST_REPATCH_MS.load(Ordering::Relaxed)) >= REPATCH_LOG_INTERVAL_MS {
        LAST_REPATCH_MS.store(now, Ordering::Relaxed);
        let mut b = Buf::new();
        b.push_str("re-patched manager 0x");
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

fn log_object(label: &str, obj: usize) {
    let mut b = Buf::new();
    b.push_str(label);
    b.push_str(" 0x");
    b.push_hex(obj as u64, 0);
    log::log_buf(&b);
}

pub fn patches() -> u64 {
    PATCHES.load(Ordering::Relaxed)
}

pub fn repatches() -> u64 {
    REPATCHES.load(Ordering::Relaxed)
}

pub fn manager() -> usize {
    CUR_MANAGER.load(Ordering::Relaxed)
}

#[cfg(feature = "measure")]
include!("extra.rs");
