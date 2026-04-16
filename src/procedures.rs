//! Logika bisnis prosedur RPC.
//!
//! Prosedur dikelompokkan jadi tiga: generik, e-commerce, dan IoT. Preset di
//! frontend memetakan skenario ke prosedur di sini.

use crate::util::now_ts;

pub fn execute_procedure(procedure: &str, arguments: &[String]) -> String {
    match procedure {
        // Prosedur generik
        "add" => {
            let sum: f64 = arguments.iter().filter_map(|a| a.parse::<f64>().ok()).sum();
            format!("{}", sum)
        }
        "multiply" => {
            let product: f64 = arguments
                .iter()
                .filter_map(|a| a.parse::<f64>().ok())
                .product();
            format!("{}", product)
        }
        "concat" => arguments.join(""),
        "uppercase" => arguments.join(" ").to_uppercase(),
        "length" => format!("{}", arguments.join(" ").len()),

        // Skenario e-commerce
        "validate_card" => validate_card(arguments),
        "check_inventory" => check_inventory(arguments),
        "calculate_shipping" => calculate_shipping(arguments),

        // Skenario IoT
        "activate_alarm" => activate_alarm(arguments),
        "read_sensor" => read_sensor(arguments),

        _ => format!("Executed {}({:?})", procedure, arguments),
    }
}

fn validate_card(arguments: &[String]) -> String {
    // Arg 0 = nomor kartu. Berlaku jika 16 digit dan Luhn check lolos.
    let number: String = arguments
        .get(0)
        .cloned()
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect();
    if number.len() != 16 {
        return "INVALID: card must be 16 digits".into();
    }
    let mut sum = 0u32;
    for (i, c) in number.chars().rev().enumerate() {
        let mut d = c.to_digit(10).unwrap_or(0);
        if i % 2 == 1 {
            d *= 2;
            if d > 9 {
                d -= 9;
            }
        }
        sum += d;
    }
    if sum % 10 == 0 {
        format!("VALID: card {}**{} accepted", &number[..4], &number[12..])
    } else {
        "INVALID: Luhn check failed".into()
    }
}

fn check_inventory(arguments: &[String]) -> String {
    // Arg 0 = product_id, Arg 1 = jumlah diminta.
    let product = arguments.get(0).cloned().unwrap_or("SKU-0".into());
    let qty: u32 = arguments.get(1).and_then(|a| a.parse().ok()).unwrap_or(1);
    // Stok deterministik berdasarkan hash sederhana dari product_id.
    let stock: u32 = product.bytes().map(|b| b as u32).sum::<u32>() % 50 + 1;
    if stock >= qty {
        format!("OK: product={} stock={} reserved={}", product, stock, qty)
    } else {
        format!(
            "OUT_OF_STOCK: product={} stock={} requested={}",
            product, stock, qty
        )
    }
}

fn calculate_shipping(arguments: &[String]) -> String {
    // Arg 0 = berat(kg), Arg 1 = jarak(km). Tarif: Rp 2000/kg + Rp 500/km.
    let weight: f64 = arguments.get(0).and_then(|a| a.parse().ok()).unwrap_or(1.0);
    let distance: f64 = arguments.get(1).and_then(|a| a.parse().ok()).unwrap_or(10.0);
    let cost = weight * 2000.0 + distance * 500.0;
    format!(
        "SHIPPING: Rp{:.0} (weight={}kg, distance={}km)",
        cost, weight, distance
    )
}

fn activate_alarm(arguments: &[String]) -> String {
    // Arg 0 = zone, Arg 1 = severity (low/high).
    let zone = arguments.get(0).cloned().unwrap_or("ZONE-A".into());
    let sev = arguments.get(1).cloned().unwrap_or("low".into());
    format!(
        "ALARM_ON: zone={} severity={} timestamp={}",
        zone,
        sev,
        now_ts()
    )
}

fn read_sensor(arguments: &[String]) -> String {
    // Arg 0 = sensor_id. Baca "nilai" deterministik.
    let sid = arguments.get(0).cloned().unwrap_or("sensor-0".into());
    let seed: u32 = sid.bytes().map(|b| b as u32).sum::<u32>();
    let value = 20.0 + (seed % 150) as f64 / 10.0; // 20.0 - 34.9
    format!("READING: sensor={} value={:.1}C", sid, value)
}
