use core::cmp::min;

// SDK offsets: ProcessEvent vtable index, UObject / UField / UStruct / UClass / UFunction.
const PROCESSEVENT_IDX: usize = 0x4C;
const OFF_NAME: usize = 0x18;
const OFF_OUTER: usize = 0x20;
const OFF_FIELD_NEXT: usize = 0x28;
const OFF_SUPER_STRUCT: usize = 0x40;
const OFF_STRUCT_CHILDREN: usize = 0x48;
const OFF_CAST_FLAGS: usize = 0xD8;
const OFF_FUNCTION_FLAGS: usize = 0xB0;
const FUNC_NATIVE: u32 = 0x400;
const MAX_SUPER_DEPTH: usize = 64;

const FNAME_BLOCKS_MAX: usize = 8192;
const FNAME_BLOCK_BYTES_MAX: usize = 0x2_0000;
const POOL_BLOCKS: usize = 0x10;
const BLOCK_SHIFT: u32 = 16;
const BLOCK_MASK: u32 = 0xFFFF;
const ENTRY_HEADER: usize = 2;
const MAX_NAME: usize = 64;

static GNAMES: AtomicUsize = AtomicUsize::new(0);

fn gnames() -> usize {
    GNAMES.load(Ordering::Relaxed)
}

fn read_fname(pool: usize, id: u32, out: &mut [u8]) -> usize {
    let block = match read_u64(pool + POOL_BLOCKS + ((id >> BLOCK_SHIFT) as usize) * 8) {
        Some(v) if v != 0 => v as usize,
        _ => return 0,
    };
    let entry = block + (id & BLOCK_MASK) as usize * 2;
    let h = match read_u16(entry) {
        Some(v) => v,
        None => return 0,
    };
    // UE 5.6 FNameEntryHeader: bIsWide:1, LowercaseProbeHash:5, Len:10
    let mut len = (h >> 6) as usize;
    let wide = (h & 1) != 0;
    if len == 0 {
        return 0;
    }
    if len > MAX_NAME {
        len = MAX_NAME;
    }
    let limit = min(out.len(), len);
    let mut buf = [0u8; MAX_NAME * 2];
    let want = min(if wide { limit * 2 } else { limit }, buf.len());
    let got = unsafe { read_raw(entry + ENTRY_HEADER, buf.as_mut_ptr(), want) };
    let mut n = 0usize;
    if wide {
        let mut i = 0usize;
        while i + 1 < got && n < limit {
            if buf[i + 1] != 0 {
                break;
            }
            let c = buf[i];
            if !(0x20..0x7f).contains(&c) {
                break;
            }
            out[n] = c;
            n += 1;
            i += 2;
        }
    } else {
        while n < limit && n < got {
            let c = buf[n];
            if !(0x20..0x7f).contains(&c) {
                break;
            }
            out[n] = c;
            n += 1;
        }
    }
    n
}

fn read_ptr(address: usize) -> usize {
    read_u64(address).unwrap_or(0) as usize
}

fn heapish(module: &Module, p: u64) -> bool {
    p != 0 && p.is_multiple_of(8) && p >= 0x1_0000 && !module.contains(p as usize)
}

fn names_world(pool: usize, world: usize) -> bool {
    let class = read_ptr(world + OFF_CLASS);
    if class == 0 {
        return false;
    }
    let id = read_u32(class + OFF_NAME).unwrap_or(0);
    let mut buf = [0u8; 8];
    let n = read_fname(pool, id, &mut buf);
    // Allow the same two suffix characters as the other name checks.
    (5..=7).contains(&n) && &buf[..5] == b"World"
}

fn discover_names(module: &Module, world: usize) -> Option<usize> {
    scan(module, 0x30, |at, b, i| {
        if rd_u32(b, i) != 0 {
            return false;
        }
        let cur_block = rd_u32(b, i + 0x08);
        let cur_cursor = rd_u32(b, i + 0x0C);
        if cur_block as usize >= FNAME_BLOCKS_MAX {
            return false;
        }
        if cur_cursor < 0x10 || cur_cursor as usize >= FNAME_BLOCK_BYTES_MAX {
            return false;
        }
        let mut k = 0usize;
        while k < 4 {
            if !heapish(module, rd_u64(b, i + POOL_BLOCKS + k * 8)) {
                return false;
            }
            k += 1;
        }
        names_world(at, world)
    })
}

fn named(address: usize, target: &str) -> bool {
    let Some(name) = read_u64(address) else {
        return false;
    };
    if name >> 32 != 0 {
        return false;
    }
    let mut buf = [0u8; MAX_NAME];
    let len = read_fname(gnames(), name as u32, &mut buf);
    len == target.len() && &buf[..len] == target.as_bytes()
}

fn find_function(module: &Module, object: usize, name: &str) -> usize {
    let mut class = read_ptr(object + OFF_CLASS);
    let mut depth = 0usize;
    while class != 0 && depth < MAX_SUPER_DEPTH {
        if !valid_object(module, class) {
            break;
        }
        let mut field = read_ptr(class + OFF_STRUCT_CHILDREN);
        let mut fields = 0usize;
        while field != 0 && fields < 8192 {
            if !valid_object(module, field) {
                break;
            }
            let field_class = read_ptr(field + OFF_CLASS);
            let cast = read_u64(field_class + OFF_CAST_FLAGS).unwrap_or(0);
            if cast & 0x80000 != 0 && named(field + OFF_NAME, name) {
                return field;
            }
            field = read_ptr(field + OFF_FIELD_NEXT);
            fields += 1;
        }
        class = read_ptr(class + OFF_SUPER_STRUCT);
        depth += 1;
    }
    0
}

type ProcessEvent = unsafe extern "system" fn(usize, usize, *mut core::ffi::c_void);

fn process_event(module: &Module, vtable: usize) -> Option<ProcessEvent> {
    let entry = read_ptr(vtable + PROCESSEVENT_IDX * 8);
    if !module.contains(entry) {
        return None;
    }
    Some(unsafe { core::mem::transmute::<usize, ProcessEvent>(entry) })
}
