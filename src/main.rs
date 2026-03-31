use gpio_cdev::{Chip, LineRequestFlags};
use std::env;
use std::path::Path;
use std::thread;
use std::time::Duration;

fn print_usage() {
    println!("Usage:");
    println!("  gpio_pin                         # Scan and list all GPIO lines (no toggle)");
    println!("  gpio_pin walk <chip> [start] [end]  # Walk through lines one by one (5s each)");
    println!("  gpio_pin scan                    # Scan and toggle all lines (original behavior)");
    println!("  gpio_pin blink <chip> <line>     # Blink a specific GPIO line repeatedly");
    println!("  gpio_pin test <chip> <line>      # Test a line with both active-high and active-low");
    println!("  gpio_pin on <chip> <line>        # Turn a specific line ON (HIGH) and hold");
    println!("  gpio_pin off <chip> <line>       # Turn a specific line OFF (LOW) and hold");
    println!();
    println!("Examples:");
    println!("  sudo ./gpio_pin                          # Just list all GPIO lines");
    println!("  sudo ./gpio_pin walk 0                   # Walk ALL lines on gpiochip0, 5s each");
    println!("  sudo ./gpio_pin walk 0 0 20              # Walk lines 0-20 on gpiochip0");
    println!("  sudo ./gpio_pin blink 0 4                # Blink gpiochip0 line 4");
    println!("  sudo ./gpio_pin test 0 4                 # Test gpiochip0 line 4 (both polarities)");
    println!("  sudo ./gpio_pin on 0 4                   # Set gpiochip0 line 4 HIGH and hold");
    println!("  sudo ./gpio_pin blink 0 4,5,6,7          # Blink multiple lines");
}

fn scan_and_list() {
    println!("=== GPIO Pin Discovery ===\n");

    let mut chip_index = 0;
    let mut total_lines = 0;

    loop {
        let chip_path = format!("/dev/gpiochip{}", chip_index);
        if !Path::new(&chip_path).exists() {
            break;
        }

        match Chip::new(&chip_path) {
            Ok(mut chip) => {
                let chip_name = chip.name().to_string();
                let chip_label = chip.label().to_string();
                let num_lines = chip.num_lines();

                println!(
                    "Found: {} ({}) — label: \"{}\" — {} lines",
                    chip_path, chip_name, chip_label, num_lines
                );

                for offset in 0..num_lines {
                    match chip.get_line(offset) {
                        Ok(line) => {
                            let info = line.info().unwrap();
                            let name = info.name().unwrap_or("unnamed").to_string();
                            let consumer = info.consumer().unwrap_or("free");
                            let direction = if info.is_kernel() {
                                "kernel"
                            } else {
                                "user/free"
                            };

                            println!(
                                "  Line {:>3}: name={:<20} consumer={:<15} [{}]",
                                offset, name, consumer, direction
                            );
                        }
                        Err(e) => {
                            println!("  Line {:>3}: <error reading: {}>", offset, e);
                        }
                    }
                }
                total_lines += num_lines;
                println!();
            }
            Err(e) => {
                println!("Could not open {}: {}", chip_path, e);
            }
        }

        chip_index += 1;
    }

    println!("=== Total: {} GPIO lines across {} chips ===", total_lines, chip_index);
}

fn scan_and_toggle_all() {
    println!("=== GPIO Pin Discovery & Toggle ALL ===\n");

    let mut chip_index = 0;
    let mut all_lines: Vec<(String, u32, String)> = Vec::new();

    loop {
        let chip_path = format!("/dev/gpiochip{}", chip_index);
        if !Path::new(&chip_path).exists() {
            break;
        }

        match Chip::new(&chip_path) {
            Ok(mut chip) => {
                let num_lines = chip.num_lines();
                println!("Found: {} — {} lines", chip_path, num_lines);

                for offset in 0..num_lines {
                    if let Ok(line) = chip.get_line(offset) {
                        let name = line.info().ok()
                            .and_then(|i| i.name().map(|n| n.to_string()))
                            .unwrap_or_else(|| "unnamed".to_string());
                        all_lines.push((chip_path.clone(), offset, name));
                    }
                }
            }
            Err(e) => println!("Could not open {}: {}", chip_path, e),
        }
        chip_index += 1;
    }

    if all_lines.is_empty() {
        println!("No GPIO lines found. Run with sudo?");
        return;
    }

    println!("\n=== Total: {} lines. Toggling each (2s ON, 500ms OFF)... ===\n", all_lines.len());
    println!("WARNING: Starting in 5 seconds. Ctrl+C to abort.");
    thread::sleep(Duration::from_secs(5));

    for (chip_path, offset, name) in &all_lines {
        print!("Toggling {} line {} ({})... ", chip_path, offset, name);

        match Chip::new(chip_path) {
            Ok(mut chip) => match chip.get_line(*offset) {
                Ok(line) => {
                    match line.request(LineRequestFlags::OUTPUT, 0, "gpio_pin_test") {
                        Ok(handle) => {
                            // ON (HIGH)
                            match handle.set_value(1) {
                                Ok(_) => print!("ON "),
                                Err(e) => { println!("set HIGH failed: {}", e); continue; }
                            }
                            match handle.get_value() {
                                Ok(val) => print!("(read={}) ", val),
                                Err(e) => print!("(read err: {}) ", e),
                            }

                            thread::sleep(Duration::from_secs(2));

                            // OFF (LOW)
                            match handle.set_value(0) {
                                Ok(_) => println!("OFF ✓"),
                                Err(e) => println!("set LOW failed: {}", e),
                            }

                            thread::sleep(Duration::from_millis(500));
                        }
                        Err(e) => println!("SKIP ({})", e),
                    }
                }
                Err(e) => println!("SKIP (line error: {})", e),
            },
            Err(e) => println!("SKIP (chip error: {})", e),
        }
    }

    println!("\n=== Done ===");
}

fn parse_lines(line_str: &str) -> Vec<u32> {
    line_str.split(',')
        .filter_map(|s| s.trim().parse::<u32>().ok())
        .collect()
}

fn blink_lines(chip_num: u32, lines: Vec<u32>) {
    let chip_path = format!("/dev/gpiochip{}", chip_num);
    println!("=== Blinking {} lines {:?} ===", chip_path, lines);
    println!("Each line: 3s ON → 3s OFF, repeating. Ctrl+C to stop.\n");

    // For the schematic: 74LVC244 buffer (non-inverting) → 220Ω → LED → GND
    // So HIGH = LED ON, LOW = LED OFF (active-high)

    loop {
        for &line_num in &lines {
            match Chip::new(&chip_path) {
                Ok(mut chip) => match chip.get_line(line_num) {
                    Ok(line) => {
                        match line.request(LineRequestFlags::OUTPUT, 0, "gpio_blink") {
                            Ok(handle) => {
                                // Turn ON (HIGH — active-high for your LED circuit)
                                println!("[Line {}] Setting HIGH (LED should turn ON)...", line_num);
                                if let Err(e) = handle.set_value(1) {
                                    println!("[Line {}] Failed to set HIGH: {}", line_num, e);
                                    continue;
                                }
                                match handle.get_value() {
                                    Ok(val) => println!("[Line {}] Read back: {}", line_num, val),
                                    Err(e) => println!("[Line {}] Read error: {}", line_num, e),
                                }

                                thread::sleep(Duration::from_secs(3));

                                // Turn OFF (LOW)
                                println!("[Line {}] Setting LOW (LED should turn OFF)...", line_num);
                                if let Err(e) = handle.set_value(0) {
                                    println!("[Line {}] Failed to set LOW: {}", line_num, e);
                                }

                                thread::sleep(Duration::from_secs(3));
                            }
                            Err(e) => println!("[Line {}] Could not request: {}", line_num, e),
                        }
                    }
                    Err(e) => println!("[Line {}] Get line error: {}", line_num, e),
                },
                Err(e) => println!("Chip error: {}", e),
            }
        }
    }
}

fn test_line(chip_num: u32, line_num: u32) {
    let chip_path = format!("/dev/gpiochip{}", chip_num);
    println!("=== Testing {} line {} with BOTH polarities ===\n", chip_path, line_num);

    match Chip::new(&chip_path) {
        Ok(mut chip) => match chip.get_line(line_num) {
            Ok(line) => {
                let info = line.info().unwrap();
                println!("Line info: name={}, consumer={}, is_kernel={}",
                    info.name().unwrap_or("unnamed"),
                    info.consumer().unwrap_or("free"),
                    info.is_kernel()
                );

                match line.request(LineRequestFlags::OUTPUT, 0, "gpio_test") {
                    Ok(handle) => {
                        // Test 1: Active-HIGH (your schematic's expected behavior)
                        println!("\n--- Test 1: Active-HIGH (set 1 = LED ON) ---");
                        println!("Setting HIGH... watch the LED for 5 seconds");
                        handle.set_value(1).unwrap();
                        println!("Read back: {}", handle.get_value().unwrap_or(99));
                        thread::sleep(Duration::from_secs(5));

                        println!("Setting LOW...");
                        handle.set_value(0).unwrap();
                        println!("Read back: {}", handle.get_value().unwrap_or(99));
                        thread::sleep(Duration::from_secs(2));

                        // Test 2: Active-LOW (in case the buffer inverts or OE is different)
                        println!("\n--- Test 2: Active-LOW (set 0 = LED ON) ---");
                        println!("Setting LOW... watch the LED for 5 seconds");
                        handle.set_value(0).unwrap();
                        println!("Read back: {}", handle.get_value().unwrap_or(99));
                        thread::sleep(Duration::from_secs(5));

                        println!("Setting HIGH...");
                        handle.set_value(1).unwrap();
                        println!("Read back: {}", handle.get_value().unwrap_or(99));
                        thread::sleep(Duration::from_secs(2));

                        // Test 3: Rapid blink (unmistakable)
                        println!("\n--- Test 3: Rapid blink (10 cycles, 500ms each) ---");
                        for i in 0..10 {
                            handle.set_value(1).unwrap();
                            println!("  Cycle {}: HIGH", i + 1);
                            thread::sleep(Duration::from_millis(500));
                            handle.set_value(0).unwrap();
                            println!("  Cycle {}: LOW", i + 1);
                            thread::sleep(Duration::from_millis(500));
                        }

                        // Leave it OFF
                        handle.set_value(0).unwrap();
                        println!("\n=== Test complete. Line set LOW. ===");
                    }
                    Err(e) => println!("Could not request line: {}", e),
                }
            }
            Err(e) => println!("Could not get line {}: {}", line_num, e),
        },
        Err(e) => println!("Could not open {}: {}", chip_path, e),
    }
}

fn hold_line(chip_num: u32, line_num: u32, value: u8) {
    let chip_path = format!("/dev/gpiochip{}", chip_num);
    let state = if value == 1 { "ON (HIGH)" } else { "OFF (LOW)" };
    println!("=== Setting {} line {} to {} ===", chip_path, line_num, state);
    println!("Holding until Ctrl+C...\n");

    match Chip::new(&chip_path) {
        Ok(mut chip) => match chip.get_line(line_num) {
            Ok(line) => {
                match line.request(LineRequestFlags::OUTPUT, value, "gpio_hold") {
                    Ok(handle) => {
                        handle.set_value(value).unwrap();
                        println!("Set to {}. Read back: {}", value, handle.get_value().unwrap_or(99));
                        println!("Holding... press Ctrl+C to release.");

                        // Hold forever until Ctrl+C
                        loop {
                            thread::sleep(Duration::from_secs(60));
                        }
                    }
                    Err(e) => println!("Could not request line: {}", e),
                }
            }
            Err(e) => println!("Could not get line: {}", e),
        },
        Err(e) => println!("Could not open chip: {}", e),
    }
}

fn walk_lines(chip_num: u32, start: u32, end: u32) {
    let chip_path = format!("/dev/gpiochip{}", chip_num);

    // First, figure out how many lines the chip has
    let max_lines = match Chip::new(&chip_path) {
        Ok(chip) => chip.num_lines(),
        Err(e) => {
            println!("Could not open {}: {}", chip_path, e);
            return;
        }
    };

    let end = end.min(max_lines - 1);
    if start > end {
        println!("Invalid range: start ({}) > end ({})", start, end);
        return;
    }

    // Phase 1: Quick scan to find which lines are actually requestable
    println!("=== Scanning {} lines {} to {} for usable pins... ===\n", chip_path, start, end);
    let mut usable_lines: Vec<u32> = Vec::new();

    for line_num in start..=end {
        match Chip::new(&chip_path) {
            Ok(mut chip) => {
                if let Ok(line) = chip.get_line(line_num) {
                    if let Ok(handle) = line.request(LineRequestFlags::OUTPUT, 0, "gpio_scan") {
                        // Can request it — it's usable
                        let _ = handle.set_value(0); // reset to LOW
                        usable_lines.push(line_num);
                    }
                }
            }
            _ => {}
        }
    }

    println!("Found {} usable lines out of {} total (skipped {} locked/reserved)",
        usable_lines.len(), end - start + 1, (end - start + 1) as usize - usable_lines.len());
    println!("Usable lines: {:?}\n", usable_lines);

    if usable_lines.is_empty() {
        println!("No usable lines in this range.");
        return;
    }

    let total = usable_lines.len();
    let eta_secs = total * 3; // 2s ON + 1s OFF per line
    println!("Will test {} lines, ~2s each = ~{} seconds total", total, eta_secs);
    println!("Watch the front panel LEDs! Note the line number when one lights up.");
    println!("Ctrl+C to stop.\n");
    thread::sleep(Duration::from_secs(2));

    // Phase 2: Walk only the usable lines
    for (i, &line_num) in usable_lines.iter().enumerate() {
        print!("[{}/{}] Line {:>3} — ", i + 1, total, line_num);

        match Chip::new(&chip_path) {
            Ok(mut chip) => match chip.get_line(line_num) {
                Ok(line) => {
                    match line.request(LineRequestFlags::OUTPUT, 0, "gpio_walk") {
                        Ok(handle) => {
                            // Set HIGH
                            if let Err(e) = handle.set_value(1) {
                                println!("HIGH failed: {}", e);
                                continue;
                            }
                            let readback = handle.get_value().unwrap_or(99);
                            println!("HIGH (read={}) — watching 2s...", readback);

                            thread::sleep(Duration::from_secs(2));

                            // Set LOW
                            handle.set_value(0).ok();

                            thread::sleep(Duration::from_millis(500));
                        }
                        Err(_) => {
                            println!("SKIP");
                        }
                    }
                }
                Err(_) => println!("SKIP"),
            },
            Err(_) => println!("SKIP"),
        }
    }

    println!("\n=== Walk complete (lines {} to {}) ===", start, end);
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        // Default: just scan and list, no toggling
        scan_and_list();
        println!("\nRun with 'scan' to toggle all, or 'blink <chip> <line>' to target specific lines.");
        println!("Run with --help for all options.");
        return;
    }

    match args[1].as_str() {
        "--help" | "-h" | "help" => {
            print_usage();
        }
        "walk" => {
            if args.len() < 3 {
                println!("Usage: gpio_pin walk <chip_number> [start_line] [end_line]");
                println!("Example: gpio_pin walk 0          # all lines on chip 0");
                println!("Example: gpio_pin walk 0 0 20     # lines 0-20 on chip 0");
                return;
            }
            let chip_num: u32 = args[2].parse().expect("chip must be a number");
            let start: u32 = if args.len() > 3 { args[3].parse().unwrap_or(0) } else { 0 };
            let end: u32 = if args.len() > 4 { args[4].parse().unwrap_or(u32::MAX) } else { u32::MAX };
            walk_lines(chip_num, start, end);
        }
        "scan" => {
            scan_and_toggle_all();
        }
        "blink" => {
            if args.len() < 4 {
                println!("Usage: gpio_pin blink <chip_number> <line_number(s)>");
                println!("Example: gpio_pin blink 0 4");
                println!("Example: gpio_pin blink 0 4,5,6,7");
                return;
            }
            let chip_num: u32 = args[2].parse().expect("chip must be a number");
            let lines = parse_lines(&args[3]);
            if lines.is_empty() {
                println!("No valid line numbers provided");
                return;
            }
            blink_lines(chip_num, lines);
        }
        "test" => {
            if args.len() < 4 {
                println!("Usage: gpio_pin test <chip_number> <line_number>");
                return;
            }
            let chip_num: u32 = args[2].parse().expect("chip must be a number");
            let line_num: u32 = args[3].parse().expect("line must be a number");
            test_line(chip_num, line_num);
        }
        "on" => {
            if args.len() < 4 {
                println!("Usage: gpio_pin on <chip_number> <line_number>");
                return;
            }
            let chip_num: u32 = args[2].parse().expect("chip must be a number");
            let line_num: u32 = args[3].parse().expect("line must be a number");
            hold_line(chip_num, line_num, 1);
        }
        "off" => {
            if args.len() < 4 {
                println!("Usage: gpio_pin off <chip_number> <line_number>");
                return;
            }
            let chip_num: u32 = args[2].parse().expect("chip must be a number");
            let line_num: u32 = args[3].parse().expect("line must be a number");
            hold_line(chip_num, line_num, 0);
        }
        _ => {
            println!("Unknown command: {}", args[1]);
            print_usage();
        }
    }
}
