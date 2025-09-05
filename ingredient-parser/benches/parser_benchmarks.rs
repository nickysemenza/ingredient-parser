use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ingredient::IngredientParser;

fn benchmark_ingredient_parsing(c: &mut Criterion) {
    // Test cases with different complexity levels
    let test_cases = vec![
        ("simple", "2 cups flour"),
        ("fraction", "1½ cups milk"),
        ("range", "2-3 tablespoons olive oil"),
        ("multiple_units", "1 cup / 240ml water"),
        ("complex", "2¼ cups all-purpose flour, sifted"),
        ("very_complex", "1½-2 pounds / 680-907g ground beef, 85% lean"),
        ("with_modifier", "3 large eggs, room temperature, beaten"),
        ("long_name", "extra-virgin cold-pressed organic olive oil"),
        ("text_numbers", "one cup whole milk"),
        ("unicode_fraction", "⅓ cup brown sugar"),
    ];

    let mut group = c.benchmark_group("ingredient_parsing");
    
    for (name, input) in &test_cases {
        // Benchmark regular parsing
        group.bench_with_input(
            BenchmarkId::new("from_str", name),
            input,
            |b, input| {
                let parser = IngredientParser::new(false);
                b.iter(|| parser.clone().from_str(black_box(input)))
            },
        );
        
        // Benchmark rich text parsing
        group.bench_with_input(
            BenchmarkId::new("from_str_rich", name), 
            input,
            |b, input| {
                let rich_parser = IngredientParser::new(true);
                b.iter(|| rich_parser.clone().from_str(black_box(input)))
            },
        );
        
        // Benchmark error handling path
        group.bench_with_input(
            BenchmarkId::new("try_from_str", name),
            input,
            |b, input| {
                b.iter(|| {
                    let parser = IngredientParser::new(false);
                    parser.try_from_str(black_box(input))
                })
            },
        );
    }
    
    group.finish();
}

fn benchmark_amount_parsing(c: &mut Criterion) {
    let amount_cases = vec![
        ("single", "2 cups"),
        ("fraction", "1½ tablespoons"),
        ("range", "2-3 ounces"),
        ("multiple", "1 cup / 240ml"),
        ("decimal", "2.5 grams"),
        ("text_number", "one teaspoon"),
    ];
    
    let mut group = c.benchmark_group("amount_parsing");
    
    for (name, input) in &amount_cases {
        group.bench_with_input(
            BenchmarkId::new("parse_amount", name),
            input,
            |b, input| {
                let parser = IngredientParser::new(false);
                b.iter(|| parser.clone().parse_amount(black_box(input)))
            },
        );
        
        group.bench_with_input(
            BenchmarkId::new("must_parse_amount", name),
            input,
            |b, input| {
                let parser = IngredientParser::new(false);
                b.iter(|| parser.clone().must_parse_amount(black_box(input)))
            },
        );
    }
    
    group.finish();
}

fn benchmark_parsing_vs_creation(c: &mut Criterion) {
    let test_input = "2¼ cups all-purpose flour, sifted";
    
    c.bench_function("create_parser", |b| {
        b.iter(|| IngredientParser::new(black_box(false)))
    });
    
    c.bench_function("parse_with_existing", |b| {
        let parser = IngredientParser::new(false);
        b.iter(|| parser.clone().from_str(black_box(test_input)))
    });
    
    c.bench_function("parse_with_creation", |b| {
        b.iter(|| {
            let parser = IngredientParser::new(false);
            parser.from_str(black_box(test_input))
        })
    });
}

fn benchmark_batch_parsing(c: &mut Criterion) {
    // Simulate parsing a recipe with multiple ingredients
    let recipe_ingredients = vec![
        "2 cups all-purpose flour",
        "1 teaspoon baking powder", 
        "½ teaspoon salt",
        "1 cup granulated sugar",
        "½ cup unsalted butter, softened",
        "2 large eggs",
        "1 teaspoon vanilla extract",
        "1 cup whole milk",
        "2 tablespoons vegetable oil",
        "1-2 tablespoons powdered sugar for dusting",
    ];
    
    c.bench_function("batch_parse_recipe", |b| {
        let parser = IngredientParser::new(false);
        b.iter(|| {
            for ingredient in &recipe_ingredients {
                parser.clone().from_str(black_box(ingredient));
            }
        })
    });
    
    c.bench_function("batch_parse_with_creation", |b| {
        b.iter(|| {
            for ingredient in &recipe_ingredients {
                let parser = IngredientParser::new(false);
                parser.from_str(black_box(ingredient));
            }
        })
    });
}

criterion_group!(
    benches,
    benchmark_ingredient_parsing,
    benchmark_amount_parsing, 
    benchmark_parsing_vs_creation,
    benchmark_batch_parsing
);
criterion_main!(benches);