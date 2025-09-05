# Benchmarking Infrastructure

This repository includes comprehensive benchmarking infrastructure using [Criterion.rs](https://github.com/bheisler/criterion.rs) and [GitHub Action Benchmark](https://github.com/benchmark-action/github-action-benchmark).

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

## GitHub Actions Integration

The repository includes three benchmark workflows:

### 1. Main Benchmark (benchmark.yml)
- Runs on pushes to `main` and pull requests
- Stores results and tracks performance over time
- Creates alerts when performance degrades >200%
- Auto-pushes results to `gh-pages` branch

### 2. PR Benchmark (benchmark-pr.yml)  
- Runs detailed benchmarks on pull requests
- Compares performance against the main branch
- Posts performance comparison as PR comment
- Alerts if performance degrades >150%

### 3. Benchmark Dashboard (benchmark-dashboard.yml)
- Updates the GitHub Pages benchmark dashboard
- Provides historical performance visualization
- Can be triggered manually via workflow dispatch

## Benchmark Dashboard

Once set up, the benchmark results are available at:
`https://<username>.github.io/<repository>/dev/bench/`

The dashboard shows:
- Performance trends over time
- Comparison between different benchmark runs
- Interactive charts with detailed statistics
- Regression detection and alerts

## Setup Requirements

### Repository Settings
1. **Enable GitHub Pages**:
   - Go to Settings → Pages
   - Set Source to "GitHub Actions"

2. **Branch Protection** (recommended):
   - Require benchmark checks to pass before merging

### Local Development
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

## Interpreting Results

### Local Results
- **Time**: Lower is better
- **Change**: Shows performance change from previous run
- **Throughput**: Higher is better for throughput measurements

### CI Results  
- **Green**: Performance within acceptable range
- **Yellow**: Performance degraded but within alert threshold
- **Red**: Performance degraded beyond alert threshold

### Performance Targets
- **Parse simple ingredients**: < 1μs
- **Parse complex ingredients**: < 5μs  
- **Batch parse recipe (10 ingredients)**: < 50μs
- **Parser creation**: < 100ns

## Troubleshooting

### Common Issues
1. **Benchmarks too slow**: Reduce sample size in criterion configuration
2. **Inconsistent results**: Check for background processes affecting CPU
3. **CI failures**: Verify GitHub Pages is enabled and permissions are set
4. **Missing baseline**: Run `cargo bench` locally to establish baseline

### Performance Regression Investigation
1. Check the benchmark dashboard for historical trends
2. Compare specific commit ranges using git bisect
3. Profile with `cargo bench -- --profile-time=5` for detailed timing
4. Use `cargo bench -- --verbose` for additional debug information