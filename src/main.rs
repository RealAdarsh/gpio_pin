use gpio_cdev::{Chip, LineRequestFlags};
use std::path::Path;
use std::thread;
use std::time::Duration;

fn main() {
    println!("=== GPIO Pin Discovery & LED Control ===\n");

    // Scan all available GPIO chips (/dev/gpiochip0, gpiochip1, ...)
    let mut chip_index = 0;
    let mut all_lines: Vec<(String, u32, String)> = Vec::new(); // (chip_path, line_offset, line_name)

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

                            all_lines.push((chip_path.clone(), offset, name));
                        }
                        Err(e) => {
                            println!("  Line {:>3}: <error reading: {}>", offset, e);
                        }
                    }
                }
                println!();
            }
            Err(e) => {
                println!("Could not open {}: {}", chip_path, e);
            }
        }

        chip_index += 1;
    }

    if all_lines.is_empty() {
        println!("No GPIO lines found. Are you running on a system with GPIO support?");
        println!("Make sure to run with: sudo ./gpio_pin");
        return;
    }

    println!("=== Total GPIO lines found: {} ===\n", all_lines.len());

    // Ask user before toggling
    println!("WARNING: This will attempt to set each GPIO line HIGH then LOW.");
    println!("This could affect hardware! Only proceed if you know what you're doing.");
    println!("Press Ctrl+C to abort, or the program will continue in 5 seconds...\n");
    thread::sleep(Duration::from_secs(5));

    // Toggle each line one by one
    for (chip_path, offset, name) in &all_lines {
        print!(
            "Toggling {} line {} ({})... ",
            chip_path, offset, name
        );

        match Chip::new(chip_path) {
            Ok(mut chip) => match chip.get_line(*offset) {
                Ok(line) => {
                    // Try to request the line as output
                    match line.request(LineRequestFlags::OUTPUT, 0, "gpio_pin_test") {
                        Ok(handle) => {
                            // Turn ON (set HIGH)
                            match handle.set_value(1) {
                                Ok(_) => print!("ON "),
                                Err(e) => {
                                    println!("set HIGH failed: {}", e);
                                    continue;
                                }
                            }

                            // Read back value
                            match handle.get_value() {
                                Ok(val) => print!("(read={}) ", val),
                                Err(e) => print!("(read err: {}) ", e),
                            }

                            thread::sleep(Duration::from_millis(500));

                            // Turn OFF (set LOW)
                            match handle.set_value(0) {
                                Ok(_) => println!("OFF ✓"),
                                Err(e) => println!("set LOW failed: {}", e),
                            }

                            thread::sleep(Duration::from_millis(200));
                        }
                        Err(e) => {
                            println!("SKIP (could not request: {})", e);
                        }
                    }
                }
                Err(e) => println!("SKIP (line error: {})", e),
            },
            Err(e) => println!("SKIP (chip error: {})", e),
        }
    }

    println!("\n=== Done ===");
}
