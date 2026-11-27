// AHTPlayerCharacter: default character ID and land appearance flag.
const OFF_DEFAULT_CHARACTER: usize = 0x2A90;
const OFF_SHOW_MAIN_MESH: usize = 0x2F39;
// AHTAbilityCharacter::DrivingState / WaterVehicleDrivingState.
const OFF_LAND_STATE: usize = 0x1300;
const OFF_WATER_STATE: usize = 0x1528;
// HTGame_structs.hpp: FCharacterVehicleState / FCharacterWaterVehicleState.
const OFF_LAND_PHASE: usize = 0x00;
const OFF_LAND_SEAT: usize = 0x3C;
const OFF_LAND_ID: usize = 0x40;
const OFF_LAND_VEHICLE: usize = 0x48;
const OFF_WATER_VEHICLE: usize = 0x00;
const OFF_WATER_SEAT: usize = 0x08;
// AHTVirtualWaterVehicleBasePawn and its seat definition/record arrays.
const OFF_WATER_SEATS: usize = 0x0390;
const OFF_WATER_ID: usize = 0x03A0;
const OFF_WATER_SEAT_RECORDS: usize = 0x03B0;
const OFF_WATER_DEFAULT_AVATAR: usize = 0x03C4;
const MAX_WATER_SEATS: u32 = 32;
// FWaterVehicleSeat / FWaterVehicleSeatRecord / FWeakObjectPtr.
const WATER_SEAT_STRIDE: usize = 0x0180;
const OFF_WATER_SEAT_SOCKET: usize = 0x00;
const WATER_RECORD_STRIDE: usize = 0x10;
const OFF_WATER_RECORD_SEAT: usize = 0x00;
const OFF_WATER_RECORD_AVAILABLE: usize = 0x04;
const OFF_WATER_RECORD_OBJECT_INDEX: usize = 0x08;
const OFF_WATER_RECORD_OBJECT_SERIAL: usize = 0x0C;
// UObject::Index, AActor::RootComponent, and USceneComponent attachment members.
const OFF_OBJECT_INDEX: usize = 0x0C;
const OFF_ROOT_COMPONENT: usize = 0x01C8;
const OFF_ATTACH_PARENT: usize = 0x00D8;
const OFF_ATTACH_SOCKET: usize = 0x00E0;

// Use the local DrivingState seat index. SeatList and VehicleType were not
// usable on several live vehicles; the driver and passenger reported 0 and 1.
const LAND_DRIVER_SEAT: i32 = 0;
const LAND_VEHICLE_CLASS: &[u8] = b"WheeledVehiclePawn";

// EVehicleInteractState in HTGame_structs.hpp.
const START_GET_ON: u8 = 0;
const END_GET_ON: u8 = 1;
const START_GET_OFF: u8 = 2;
const END_GET_OFF: u8 = 3;
// EFunctionFlags::FUNC_Net.
const FUNC_NET: u32 = 0x40;
#[cfg(not(feature = "spine"))]
const NAME_RETRY_MS: u64 = 10_000;

#[derive(PartialEq, Eq)]
struct Ride {
    vehicle: usize,
    id: u64,
    seat: i32,
}

#[derive(PartialEq, Eq)]
struct WaterAttachment {
    parent: usize,
    socket: u64,
}

#[derive(PartialEq, Eq)]
struct WaterDriver {
    seat_socket: u64,
    occupant: u64,
    attachment: WaterAttachment,
}

#[derive(PartialEq, Eq)]
struct Sample {
    world: usize,
    controller: usize,
    possessed: usize,
    player: usize,
    default_role: u64,
    display_role: u64,
    show_main_mesh: u8,
    land_phase: u8,
    land: Ride,
    water: Ride,
    water_driver: Option<WaterDriver>,
}

impl Sample {
    fn has_appearance_override(&self) -> bool {
        self.default_role != 0 && self.display_role != 0 && self.default_role != self.display_role
    }
}

fn live_is_a(module: &Module, object: usize, target: &[u8]) -> bool {
    let Some(d) = describe(module, object) else {
        return false;
    };
    if d.flags & RF_CDO_OR_ARCHETYPE != 0 {
        return false;
    }
    let mut class = read_ptr(object + OFF_CLASS);
    let mut text = [0u8; MAX_NAME];
    for _ in 0..MAX_SUPER_DEPTH {
        if describe(module, class).is_none() {
            break;
        }
        let Some(id) = read_u32(class + OFF_NAME) else {
            break;
        };
        let len = read_fname(gnames(), id, &mut text);
        if &text[..len] == target {
            return true;
        }
        class = read_ptr(class + OFF_SUPER_STRUCT);
    }
    false
}

fn read_block<const N: usize>(address: usize) -> Option<[u8; N]> {
    let mut bytes = [0u8; N];
    if unsafe { read_raw(address, bytes.as_mut_ptr(), N) } == N {
        Some(bytes)
    } else {
        None
    }
}

fn water_attachment(module: &Module, player: usize, vehicle: usize) -> Option<WaterAttachment> {
    let root = read_ptr(player + OFF_ROOT_COMPONENT);
    if !live_is_a(module, root, b"SceneComponent") {
        return None;
    }
    let data =
        read_block::<{ OFF_ATTACH_SOCKET - OFF_ATTACH_PARENT + 8 }>(root + OFF_ATTACH_PARENT)?;
    let parent = rd_u64(&data, 0) as usize;
    let mut outer = parent;
    for _ in 0..8 {
        if outer == vehicle || describe(module, outer).is_none() {
            break;
        }
        outer = read_ptr(outer + OFF_OUTER);
    }
    (outer == vehicle).then_some(WaterAttachment {
        parent,
        socket: rd_u64(&data, OFF_ATTACH_SOCKET - OFF_ATTACH_PARENT),
    })
}

fn water_seat_socket(vehicle: usize, seat: i32) -> Option<u64> {
    let index = u32::try_from(seat).ok()?;
    let address = vehicle + OFF_WATER_SEATS;
    let (data, count) = array(address, MAX_WATER_SEATS)?;
    if index >= count || data % 8 != 0 {
        return None;
    }
    let socket = data.checked_add(index as usize * WATER_SEAT_STRIDE + OFF_WATER_SEAT_SOCKET)?;
    let name = read_u64(socket)?;
    (array(address, MAX_WATER_SEATS) == Some((data, count))).then_some(name)
}

fn water_seat_occupant(vehicle: usize, seat: i32, player: usize) -> Option<u64> {
    if seat < 0 {
        return None;
    }
    let address = vehicle + OFF_WATER_SEAT_RECORDS;
    let (data, count) = array(address, MAX_WATER_SEATS)?;
    if data % 8 != 0 {
        return None;
    }
    for i in 0..count as usize {
        let record = read_block::<WATER_RECORD_STRIDE>(data.checked_add(i * WATER_RECORD_STRIDE)?)?;
        if rd_u32(&record, OFF_WATER_RECORD_SEAT) as i32 != seat {
            continue;
        }
        if record[OFF_WATER_RECORD_AVAILABLE] != 0
            || array(address, MAX_WATER_SEATS) != Some((data, count))
        {
            return None;
        }
        let index = rd_u32(&record, OFF_WATER_RECORD_OBJECT_INDEX) as i32;
        let serial = rd_u32(&record, OFF_WATER_RECORD_OBJECT_SERIAL) as i32;
        if serial <= 0 || index < 0 || read_u32(player + OFF_OBJECT_INDEX) != Some(index as u32) {
            return None;
        }
        // Keep the weak identity for stability checks without dereferencing it.
        return Some(rd_u64(&record, OFF_WATER_RECORD_OBJECT_INDEX));
    }
    None
}

fn water_driver(module: &Module, player: usize, ride: &Ride) -> Option<WaterDriver> {
    if ride.id == 0 || read_u8(ride.vehicle + OFF_WATER_DEFAULT_AVATAR) != Some(1) {
        return None;
    }
    let seat_socket = water_seat_socket(ride.vehicle, ride.seat)?;
    if !is_driver_socket(seat_socket) {
        return None;
    }
    let occupant = water_seat_occupant(ride.vehicle, ride.seat, player)?;
    // The seat's weak index only corroborates the live local character's
    // actual attachment to a driver socket on this boat.
    let attachment = water_attachment(module, player, ride.vehicle)?;
    is_driver_socket(attachment.socket).then_some(WaterDriver {
        seat_socket,
        occupant,
        attachment,
    })
}

fn sample(module: &Module, slot: usize, world: usize) -> Option<Sample> {
    let controller = local_controller(module, world)?;
    if !live_is_a(module, controller, b"HTPlayerController") {
        return None;
    }
    let possessed = read_u64(controller + OFF_CONTROLLER_PAWN)? as usize;
    let owner = read_u64(controller + OFF_PC_HT_PLAYER_OWNER)? as usize;
    // Prefer the controlled character. A vehicle pawn may temporarily be
    // possessed instead; HTPlayerOwner still points to the local character.
    let player = [possessed, owner]
        .into_iter()
        .find(|&object| live_is_a(module, object, b"HTPlayerCharacter"))?;
    let roles = read_block::<{ OFF_CURRENT_CHARACTER_ID - OFF_DEFAULT_CHARACTER + 8 }>(
        player + OFF_DEFAULT_CHARACTER,
    )?;
    let show_main_mesh = read_u8(player + OFF_SHOW_MAIN_MESH)?;
    let land = read_block::<{ OFF_LAND_VEHICLE + 8 }>(player + OFF_LAND_STATE)?;
    let water = read_block::<{ OFF_WATER_SEAT + 4 }>(player + OFF_WATER_STATE)?;
    let water_vehicle = rd_u64(&water, OFF_WATER_VEHICLE) as usize;
    let water = Ride {
        vehicle: water_vehicle,
        id: if live_is_a(module, water_vehicle, b"HTVirtualWaterVehicleBasePawn") {
            read_u64(water_vehicle + OFF_WATER_ID).unwrap_or(0)
        } else {
            0
        },
        seat: rd_u32(&water, OFF_WATER_SEAT) as i32,
    };
    let water_driver = water_driver(module, player, &water);

    // Discard a sample collected across a world change or a pawn handoff.
    if read_ptr(slot) != world
        || read_u64(controller + OFF_CONTROLLER_PAWN)? as usize != possessed
        || read_u64(controller + OFF_PC_HT_PLAYER_OWNER)? as usize != owner {
        return None;
    }
    Some(Sample {
        world,
        controller,
        possessed,
        player,
        default_role: rd_u64(&roles, 0),
        display_role: rd_u64(&roles, OFF_CURRENT_CHARACTER_ID - OFF_DEFAULT_CHARACTER),
        show_main_mesh,
        land_phase: land[OFF_LAND_PHASE],
        land: Ride {
            vehicle: rd_u64(&land, OFF_LAND_VEHICLE) as usize,
            id: rd_u64(&land, OFF_LAND_ID),
            seat: rd_u32(&land, OFF_LAND_SEAT) as i32,
        },
        water,
        water_driver,
    })
}

fn same_local_character(a: &Sample, b: &Sample) -> bool {
    a.world == b.world
        && a.controller == b.controller
        && a.possessed == b.possessed
        && a.player == b.player
        && a.default_role == b.default_role
}

fn is_driver_socket(socket: u64) -> bool {
    if socket == 0 || socket >> 32 != 0 {
        return false;
    }
    let mut text = [0u8; MAX_NAME];
    let len = read_fname(gnames(), socket as u32, &mut text);
    text[..len].eq_ignore_ascii_case(b"Driver")
}

#[derive(Default)]
struct WaterRefresh {
    armed: Option<(usize, usize, usize)>,
}

impl WaterRefresh {
    fn tick(&mut self, module: &Module, slot: usize, current: Option<&Sample>, changed: bool) {
        let Some(s) = current else {
            return;
        };
        let context = (s.world, s.controller, s.player);
        if s.water.vehicle == 0 {
            self.armed = (s.possessed == s.player).then_some(context);
            return;
        }
        // Water has no persistent EndGetOn phase; require two matching ticks.
        if changed
            || self.armed != Some(context)
            || s.land.vehicle != 0
            || s.possessed != s.player
            || !s.has_appearance_override()
            || s.water_driver.is_none()
        {
            return;
        }

        self.armed = None;
        refresh_display(module, slot, s, |fresh| fresh == s);
    }
}

fn refresh_land(module: &Module, slot: usize, previous: Option<&Sample>, seated: &Sample) {
    let observed_boarding = previous.is_some_and(|s| {
        same_local_character(s, seated)
            && s.water.vehicle == 0
            && (s.land.vehicle == 0 || (s.land_phase == START_GET_ON && s.land == seated.land))
    });
    if seated.land.seat != LAND_DRIVER_SEAT
        || seated.possessed != seated.player
        || seated.water.vehicle != 0
        || seated.show_main_mesh != 1
        || !seated.has_appearance_override()
        || !observed_boarding
        || !live_is_a(module, seated.land.vehicle, LAND_VEHICLE_CLASS) {
        return;
    }
    refresh_display(module, slot, seated, |s| {
        same_local_character(s, seated)
            && s.land == seated.land
            && s.land_phase == END_GET_ON
            && s.water.vehicle == 0
            && s.show_main_mesh == 1
            && s.display_role == seated.display_role
            && live_is_a(module, s.land.vehicle, LAND_VEHICLE_CLASS)
    });
}

fn refresh_display(
    module: &Module,
    slot: usize,
    seated: &Sample,
    still_seated: impl FnOnce(&Sample) -> bool,
) {
    let function = find_function(module, seated.player, "DisplayCurrentCharacter");
    if function == 0
        || !named(read_ptr(function + OFF_OUTER) + OFF_NAME, "HTPlayerCharacter")
        || !read_u32(function + OFF_FUNCTION_FLAGS)
            .is_some_and(|flags| flags & (FUNC_NATIVE | FUNC_NET) == FUNC_NATIVE) {
        log::line("vehicle: native DisplayCurrentCharacter not verified");
        return;
    }

    // Recheck after function lookup in case the local driver has changed.
    if !sample(module, slot, seated.world).as_ref().is_some_and(still_seated) {
        return;
    }
    let Some(invoke) = describe(module, seated.player).and_then(|d| process_event(module, d.vtable))
    else {
        log::line("vehicle: ProcessEvent not available");
        return;
    };
    unsafe { invoke(seated.player, function, core::ptr::null_mut()) };
}

#[derive(Default)]
struct VehicleState {
    previous: Option<Sample>,
    handled_land_ride: bool,
    water_refresh: WaterRefresh,
    #[cfg(not(feature = "spine"))]
    last_name_scan: Option<u64>,
}

impl VehicleState {
    fn tick(&mut self, module: &Module) {
        let slot = WORLD_SLOT.load(Ordering::Relaxed);
        let world = read_ptr(slot);
        #[cfg(not(feature = "spine"))]
        if gnames() == 0 && world != 0 {
            let now = unsafe { ffi::GetTickCount64() };
            if self
                .last_name_scan
                .is_none_or(|last| now.wrapping_sub(last) >= NAME_RETRY_MS)
                && local_controller(module, world).is_some() {
                self.last_name_scan = Some(now);
                if let Some(pool) = discover_names(module, world) {
                    GNAMES.store(pool, Ordering::Relaxed);
                }
            }
        }
        let current = if world != 0 && gnames() != 0 {
            sample(module, slot, world)
        } else {
            None
        };
        let changed = current != self.previous;
        if let Some(s) = current.as_ref().filter(|_| changed) {
            if s.land.vehicle == 0 || matches!(s.land_phase, START_GET_OFF | END_GET_OFF) {
                self.handled_land_ride = false;
            }
            if !self.handled_land_ride
                && s.land_phase == END_GET_ON
                && s.land.vehicle != 0
                && s.land.id != 0
                && s.water.vehicle == 0
            {
                self.handled_land_ride = true;
                refresh_land(module, slot, self.previous.as_ref(), s);
            }
        }
        self.water_refresh.tick(module, slot, current.as_ref(), changed);
        self.previous = current;
    }
}
