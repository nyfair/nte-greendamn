use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use crate::buf::Buf;
use crate::log;
use crate::log_buf;
use crate::mem::{self, Module};

// offset from Dumper-7 CppSDK
// class UCurveFloat*                            ActorFadeCurve;                                    // 0x37C8(0x0008)(Edit, BlueprintVisible, ZeroConstructor, NoDestructor, HasGetValueTypeHash, NativeAccessSpecifierPublic)
const ACTOR_FADE_CURVE: usize = 0x37C8;
// float                                         PlayerFadeSpeed;                                   // 0x3B30(0x0004)(Edit, ZeroConstructor, IsPlainOldData, NoDestructor, HasGetValueTypeHash, NativeAccessSpecifierPrivate)
const PLAYER_FADE_SPEED: usize = 0x3B30;
// float                                         PlayerFadeDistanceSquare;                          // 0x3B34(0x0004)(Edit, ZeroConstructor, IsPlainOldData, NoDestructor, HasGetValueTypeHash, NativeAccessSpecifierPrivate)
const PLAYER_FADE_DISTANCE_SQUARE: usize = 0x3B34;
// float                                         PlayerHideDistanceSquare;                          // 0x3B38(0x0004)(Edit, ZeroConstructor, IsPlainOldData, NoDestructor, HasGetValueTypeHash, NativeAccessSpecifierPrivate)pub const PLAYER_HIDE_DISTANCE_SQUARE: usize = 0x3B38;
const PLAYER_HIDE_DISTANCE_SQUARE: usize = 0x3B38;
// class UCurveFloat*                            PlayerPitchFadeCurve;                              // 0x3B40(0x0008)(Edit, ZeroConstructor, NoDestructor, HasGetValueTypeHash, NativeAccessSpecifierPrivate)
const PLAYER_PITCH_FADE_CURVE: usize = 0x3B40;
// constexpr int32 GObjects          = 0x0F4D9C80;    GUObjectArray RVA -> GObjects - 0x10;
const GUOBJ_RVA_HINT: usize = 0xF4D9C70;

const EXPECT_SPEED: f32 = 2.;
const EXPECT_FADE_DIST: f32 = 10000.;
const EXPECT_HIDE_DIST: f32 = 6400.;
const FUOBJECTITEM_SIZE: usize = 24;
const ELEMS_PER_CHUNK: usize = 0x10000;
const OFF_OBJECTS: usize = 0x10;
const OFF_NUM_ELEMENTS: usize = 0x24;
const OFF_NUM_CHUNKS: usize = 0x2C;
const RECORDS_PER_PIECE: usize = 4096;
const PIECE: usize = RECORDS_PER_PIECE * FUOBJECTITEM_SIZE;
const RF_CDO_OR_ARCHETYPE: u32 = 0x10 | 0x20;

static GUOBJ: AtomicUsize = AtomicUsize::new(0);
static GUOBJ_CONFIRMED: AtomicU32 = AtomicU32::new(0);
static LAST_INDEX: AtomicU32 = AtomicU32::new(0);
static ATTEMPT: AtomicU32 = AtomicU32::new(0);
// vtable of the class we patch, learned at runtime (0 = not learned yet)
static TARGET: AtomicUsize = AtomicUsize::new(0);

struct Stats {
    elems: u64,
    same_class: u64,
    clean: u64,
    mismatch: u64,
    patched: usize,
    cdo: u64,
}

pub fn recon(module: &Module) -> usize {
    let attempt = ATTEMPT.fetch_add(1, Ordering::Relaxed) + 1;
    let Some(guobj) = guobj_addr(module) else {
        return 0;
    };
    let num_elements = match mem::read_u32(guobj + OFF_NUM_ELEMENTS) {
        Some(v) => v,
        None => return 0,
    };
    let num_chunks = match mem::read_u32(guobj + OFF_NUM_CHUNKS) {
        Some(v) => v,
        None => return 0,
    };

    let from = LAST_INDEX.load(Ordering::Relaxed);
    if num_elements <= from {
        LAST_INDEX.store(num_elements, Ordering::Relaxed);
        return 0;
    }
    let to = num_elements;

    let mut stats = Stats {
        elems: 0,
        same_class: 0,
        clean: 0,
        mismatch: 0,
        patched: 0,
        cdo: 0,
    };
    enumerate(module, guobj, num_chunks, from, to, &mut stats);
    LAST_INDEX.store(num_elements, Ordering::Relaxed);

    if stats.same_class > 0 || stats.cdo > 0 {
        let mut b = Buf::new();
        b.push_str("pass ");
        b.push_u64(attempt as u64);
        b.push_str(" | array [");
        b.push_u64(from as u64);
        b.push_str(", ");
        b.push_u64(to as u64);
        b.push_str(") | checked ");
        b.push_u64(stats.elems);
        b.push_str(" | same class ");
        b.push_u64(stats.same_class);
        b.push_str(" (clean ");
        b.push_u64(stats.clean);
        b.push_str(", CDO ");
        b.push_u64(stats.cdo);
        b.push_str(", mismatch ");
        b.push_u64(stats.mismatch);
        b.push_str(") | rewrote ");
        b.push_u64(stats.patched as u64);
        log_buf(&b);
    }

    stats.patched
}

fn enumerate(
    module: &Module,
    guobj: usize,
    num_chunks: u32,
    from: u32,
    to: u32,
    stats: &mut Stats,
) {
    let objects_ptr = match mem::read_u64(guobj + OFF_OBJECTS) {
        Some(v) if v != 0 => v as usize,
        _ => return,
    };

    let mut ci = (from as usize) / ELEMS_PER_CHUNK;
    let chunk_limit = num_chunks as usize;
    let mut buf = [0u8; PIECE];

    while ci < chunk_limit {
        let chunk_base_index = ci * ELEMS_PER_CHUNK;
        if chunk_base_index >= to as usize {
            break;
        }

        let chunk = match mem::read_u64(objects_ptr + ci * 8) {
            Some(v) if v != 0 => v as usize,
            _ => {
                ci += 1;
                continue;
            }
        };

        let lo = if chunk_base_index < from as usize {
            from as usize - chunk_base_index
        } else {
            0
        };
        let hi = core::cmp::min(ELEMS_PER_CHUNK, to as usize - chunk_base_index);

        let mut rec = lo;
        while rec < hi {
            let piece_base = (rec / RECORDS_PER_PIECE) * RECORDS_PER_PIECE;
            let got = unsafe {
                mem::read_raw(
                    chunk + piece_base * FUOBJECTITEM_SIZE,
                    buf.as_mut_ptr(),
                    PIECE,
                )
            };
            if got < FUOBJECTITEM_SIZE {
                rec = piece_base + RECORDS_PER_PIECE;
                continue;
            }
            let recs_in_piece = got / FUOBJECTITEM_SIZE;
            let mut r = rec - piece_base;
            while r < recs_in_piece {
                let idx = piece_base + r;
                if idx >= hi {
                    break;
                }
                let base = r * FUOBJECTITEM_SIZE;
                let obj = u64_at(&buf[base..base + 8]);
                if obj != 0 {
                    stats.elems += 1;
                    stats.patched += handle(module, obj as usize, stats);
                }
                r += 1;
            }
            rec = piece_base + recs_in_piece;
        }
        ci += 1;
    }
}

fn u64_at(b: &[u8]) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[..8]);
    u64::from_ne_bytes(a)
}

fn handle(module: &Module, obj: usize, stats: &mut Stats) -> usize {
    let vtable = match mem::read_u64(obj) {
        Some(v) => v as usize,
        None => return 0,
    };
    if !module.contains(vtable) {
        return 0;
    }
    if is_cdo_or_archetype(obj) {
        stats.cdo += 1;
        return 0;
    }

    let known = TARGET.load(Ordering::Relaxed);
    if known == 0 {
        // the field values the game ships are the signature,
        // and the vtable of the first object carrying them is the one to match from now on
        if !shipped_values(obj) {
            return 0;
        }
        TARGET.store(vtable, Ordering::Relaxed);
        log_learned(module, vtable);
    } else if vtable != known {
        return 0;
    }

    stats.same_class += 1;
    if !vtable_like(module, vtable) {
        return 0;
    }
    if is_clean(obj) {
        stats.clean += 1;
        return 0;
    }
    if !shipped_values(obj) {
        log_mismatch(obj);
        stats.mismatch += 1;
        return 0;
    }

    log_object(obj);
    let ok = apply(obj);
    log_write_result(obj, ok);
    if ok {
        1
    } else {
        0
    }
}

/// true when both curve pointers are null and both distances are zeroed, i.e. the object is
/// already in the state we keep it in
fn is_clean(obj: usize) -> bool {
    mem::read_u64(obj + ACTOR_FADE_CURVE) == Some(0)
        && mem::read_u64(obj + PLAYER_PITCH_FADE_CURVE) == Some(0)
        && mem::read_f32(obj + PLAYER_FADE_DISTANCE_SQUARE) == Some(0.0)
        && mem::read_f32(obj + PLAYER_HIDE_DISTANCE_SQUARE) == Some(0.0)
}

/// true when the fade fields still hold the values the game ships them with; used both to learn
/// the class vtable and to refuse writes when the member layout no longer matches
fn shipped_values(obj: usize) -> bool {
    is_default(
        mem::read_f32(obj + PLAYER_FADE_SPEED).unwrap_or(f32::NAN),
        mem::read_f32(obj + PLAYER_FADE_DISTANCE_SQUARE).unwrap_or(f32::NAN),
        mem::read_f32(obj + PLAYER_HIDE_DISTANCE_SQUARE).unwrap_or(f32::NAN),
    )
}

fn log_learned(module: &Module, vtable: usize) {
    let mut b = Buf::new();
    b.push_str("target class learned: vtable 0x");
    b.push_hex(vtable as u64, 0);
    b.push_str(" (RVA 0x");
    b.push_hex(vtable.wrapping_sub(module.base) as u64, 0);
    b.push_str(")");
    log_buf(&b);
}

fn log_mismatch(obj: usize) {
    let mut b = Buf::new();
    b.push_str("  layout mismatch @0x");
    b.push_hex(obj as u64, 0);
    b.push_str(": values ");
    b.push_f32(mem::read_f32(obj + PLAYER_FADE_SPEED).unwrap_or(f32::NAN));
    b.push_byte(b'/');
    b.push_f32(mem::read_f32(obj + PLAYER_FADE_DISTANCE_SQUARE).unwrap_or(f32::NAN));
    b.push_byte(b'/');
    b.push_f32(mem::read_f32(obj + PLAYER_HIDE_DISTANCE_SQUARE).unwrap_or(f32::NAN));
    b.push_str(", refused to write (offsets from another game build?)");
    log_buf(&b);
}

fn is_cdo_or_archetype(obj: usize) -> bool {
    match mem::read_u32(obj + 0x08) {
        Some(f) => f & RF_CDO_OR_ARCHETYPE != 0,
        None => false,
    }
}

fn log_object(obj: usize) {
    let speed = mem::read_f32(obj + PLAYER_FADE_SPEED).unwrap_or(f32::NAN);
    let fade_dist = mem::read_f32(obj + PLAYER_FADE_DISTANCE_SQUARE).unwrap_or(f32::NAN);
    let hide_dist = mem::read_f32(obj + PLAYER_HIDE_DISTANCE_SQUARE).unwrap_or(f32::NAN);

    let mut b = Buf::new();
    b.push_str("  0x");
    b.push_hex(obj as u64, 0);
    b.push_str("  values ");
    b.push_f32(speed);
    b.push_byte(b'/');
    b.push_f32(fade_dist);
    b.push_byte(b'/');
    b.push_f32(hide_dist);
    b.push_str(if is_default(speed, fade_dist, hide_dist) {
        " (all three floats still default)"
    } else {
        " (floats already changed)"
    });
    log_buf(&b);
}

fn is_default(speed: f32, fade_dist: f32, hide_dist: f32) -> bool {
    (speed - EXPECT_SPEED).abs() < 1e-4
        && (fade_dist - EXPECT_FADE_DIST).abs() < 1e-4
        && (hide_dist - EXPECT_HIDE_DIST).abs() < 1e-4
}

fn log_write_result(obj: usize, ok: bool) {
    let mut b = Buf::new();
    b.push_str(if ok {
        "  -> wrote, read back: "
    } else {
        "  -> write failed, now: "
    });
    b.push_f32(mem::read_f32(obj + PLAYER_FADE_SPEED).unwrap_or(f32::NAN));
    b.push_byte(b'/');
    b.push_f32(mem::read_f32(obj + PLAYER_FADE_DISTANCE_SQUARE).unwrap_or(f32::NAN));
    b.push_byte(b'/');
    b.push_f32(mem::read_f32(obj + PLAYER_HIDE_DISTANCE_SQUARE).unwrap_or(f32::NAN));
    b.push_str("  curves=");
    b.push_hex(mem::read_u64(obj + ACTOR_FADE_CURVE).unwrap_or(0), 0);
    b.push_byte(b'/');
    b.push_hex(mem::read_u64(obj + PLAYER_PITCH_FADE_CURVE).unwrap_or(0), 0);
    log_buf(&b);
}

pub fn apply(obj: usize) -> bool {
    let mut ok = true;
    ok &= mem::write_u64(obj + ACTOR_FADE_CURVE, 0);
    ok &= mem::write_u64(obj + PLAYER_PITCH_FADE_CURVE, 0);
    ok &= mem::write_f32(obj + PLAYER_FADE_DISTANCE_SQUARE, 0.0);
    ok &= mem::write_f32(obj + PLAYER_HIDE_DISTANCE_SQUARE, 0.0);
    ok
}

fn guobj_addr(module: &Module) -> Option<usize> {
    if let Some(cached) = cached_guobj() {
        return Some(cached);
    }

    let hint = module.base + GUOBJ_RVA_HINT;
    if is_guobjectarray(module, hint) {
        GUOBJ.store(hint, Ordering::Relaxed);
        GUOBJ_CONFIRMED.store(1, Ordering::Relaxed);
        log::line("GUObjectArray ready (RVA hint verified)");
        return Some(hint);
    }
    None
}

fn cached_guobj() -> Option<usize> {
    let cached = GUOBJ.load(Ordering::Relaxed);
    if cached == 0 || GUOBJ_CONFIRMED.load(Ordering::Relaxed) == 0 {
        return None;
    }
    let n = mem::read_u32(cached + OFF_NUM_ELEMENTS)?;
    let c = mem::read_u32(cached + OFF_NUM_CHUNKS)?;
    let o = mem::read_u64(cached + OFF_OBJECTS)?;
    if n >= 1 && n <= 10_000_000 && c >= 1 && c <= 10_000 && o != 0 {
        Some(cached)
    } else {
        None
    }
}

fn is_guobjectarray(module: &Module, c: usize) -> bool {
    let count = match mem::read_u32(c + OFF_NUM_ELEMENTS) {
        Some(v) => v,
        None => return false,
    };
    if count < 1 || count > 10_000_000 {
        return false;
    }
    let chunks = match mem::read_u32(c + OFF_NUM_CHUNKS) {
        Some(v) => v,
        None => return false,
    };
    if chunks == 0 || chunks > 10_000 {
        return false;
    }
    let objects = match mem::read_u64(c + OFF_OBJECTS) {
        Some(v) if v != 0 => v as usize,
        _ => return false,
    };
    if module.contains(objects) {
        return false;
    }
    let chunk0 = match mem::read_u64(objects) {
        Some(v) if v != 0 => v as usize,
        _ => return false,
    };
    let mut j = 0usize;
    while j < 32 {
        if let Some(o) = mem::read_u64(chunk0 + j * FUOBJECTITEM_SIZE) {
            if o != 0 {
                if let Some(vt) = mem::read_u64(o as usize) {
                    return module.contains(vt as usize);
                }
                return false;
            }
        }
        j += 1;
    }
    false
}

pub fn vtable_like(module: &Module, vtable: usize) -> bool {
    const ENTRIES: usize = 4;
    let mut i = 0usize;
    while i < ENTRIES {
        match mem::read_u64(vtable + i * 8) {
            Some(p) if mem::is_code(module, p as usize) => {}
            _ => return false,
        }
        i += 1;
    }
    true
}
