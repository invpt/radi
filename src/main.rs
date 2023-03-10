use crate::parser::utils::ast_size;

mod char_reader;
mod errors;
mod parser;
mod string_storage;
mod tokenizer;
mod resolver;
mod parse_manager;

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let storage = string_storage::StringStorage::new();
    let toks = tokenizer::Tokens::of(
        char_reader::IoCharReader::<256, _>::new(std::fs::File::open(path).unwrap()),
        &storage,
    );
    let errs = errors::ErrorStream::new();
    let tree = parser::parse(toks, &errs).unwrap();
    //println!("{:#?}", tree);
    println!(
        "AST size: {}KiB (Expr {} bytes)",
        ast_size(&tree) / 1024,
        std::mem::size_of::<parser::Expr>()
    )
}
