# Benchmarking Infrastructure

This repository includes comprehensive benchmarking infrastructure using [Criterion.rs](https://github.com/bheisler/criterion.rs) for local performance testing.

## Running Benchmarks Locally

```bash
cd ingredient-parser

# Run all benchmarks
cargo bench

# Quick test run (less statistical rigor, faster)
cargo bench -- --test

# Run specific benchmark groups
cargo bench ingredient_parsing
cargo bench amount_parsing

# Run specific benchmark
cargo bench batch_parse_recipe

# Save baseline for comparison
cargo bench -- --save-baseline my-baseline

# Compare against baseline
cargo bench -- --baseline my-baseline
```

## Benchmark Coverage

The benchmarks test various aspects of the ingredient parser:

### Ingredient Parsing Benchmarks
- **Simple ingredients**: "2 cups flour"  
- **Fractions**: "1½ cups milk"
- **Ranges**: "2-3 tablespoons olive oil"
- **Multiple units**: "1 cup / 240ml water"
- **Complex ingredients**: "2¼ cups all-purpose flour, sifted"
- **Very complex**: "1½-2 pounds / 680-907g ground beef, 85% lean"
- **With modifiers**: "3 large eggs, room temperature, beaten"
- **Long names**: "extra-virgin cold-pressed organic olive oil"
- **Text numbers**: "one cup whole milk"
- **Unicode fractions**: "⅓ cup brown sugar"

Each test case is run against:
- `from_str()` - normal parsing
- `from_str_rich()` - rich text parsing  
- `try_from_str()` - error handling path

### Amount Parsing Benchmarks
- **Single amounts**: "2 cups"
- **Fractions**: "1½ tablespoons"  
- **Ranges**: "2-3 ounces"
- **Multiple formats**: "1 cup / 240ml"
- **Decimals**: "2.5 grams"
- **Text numbers**: "one teaspoon"

### Performance Comparison Benchmarks
- **Parser creation overhead**: `IngredientParser::new()`
- **Parser reuse vs creation**: reusing parser vs creating new ones
- **Batch parsing**: parsing multiple ingredients like a full recipe

## Local Benchmark Reports

When you run benchmarks locally, Criterion generates detailed HTML reports:

### Setup for HTML Reports
1. **Install Rust and Cargo** (if not already installed)
2. **Install gnuplot** for detailed HTML reports (optional):
   ```bash
   # macOS
   brew install gnuplot
   
   # Ubuntu/Debian  
   sudo apt-get install gnuplot
   
   # Windows
   # Download from http://www.gnuplot.info/
   ```

3. **View reports**: After running `cargo bench`, open `target/criterion/report/index.html` in your browser

## Interpreting Results

### Benchmark Output
- **Time**: Lower is better
- **Change**: Shows performance change from previous run  
- **Throughput**: Higher is better for throughput measurements
- **HTML Reports**: Detailed charts and statistical analysis

### Performance Targets
- **Parse simple ingredients**: < 1μs
- **Parse complex ingredients**: < 5μs  
- **Batch parse recipe (10 ingredients)**: < 50μs
- **Parser creation**: < 100ns

## Troubleshooting

### Common Issues
1. **Benchmarks too slow**: Reduce sample size in criterion configuration
2. **Inconsistent results**: Check for background processes affecting CPU  
3. **Missing baseline**: Run `cargo bench` locally to establish baseline
4. **No HTML reports**: Install gnuplot for visual charts

### Performance Analysis
1. **Compare baselines**: Use `cargo bench -- --save-baseline` and `--baseline` flags
2. **Profile detailed timing**: Use `cargo bench -- --profile-time=5` 
3. **Verbose output**: Use `cargo bench -- --verbose` for debug information
4. **Targeted benchmarks**: Run specific tests with `cargo bench <pattern>`