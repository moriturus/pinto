# sprint-capacity (single feature: Sprint capacity)

Dataset for Sprint capacity calculation. The seeded Sprint runs from 2026-06-01 to 2026-06-12 with six hours
per workday, four holidays, and a 20% meeting deduction.

```bash
cargo run --manifest-path ../../../Cargo.toml -- sprint capacity S-1                    # show workdays and total hours
cargo run --manifest-path ../../../Cargo.toml -- sprint capacity S-1 -j                 # JSON output
cargo run --manifest-path ../../../Cargo.toml -- sprint capacity S-1 -H 8 -d 2 -f 0.15    # update settings
```
