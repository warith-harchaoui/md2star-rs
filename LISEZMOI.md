# md2star-rs

[🇫🇷 LISEZMOI](LISEZMOI.md) · [🇬🇧 README](README.md)

**Un convertisseur Markdown → DOCX 100 % Rust. Sans Pandoc, sans sous-processus, sans
dépendance d'exécution — un seul binaire statique qui tourne sur tous les OS et appareils.**

Par [Warith HARCHAOUI](https://linkedin.com/in/warith-harchaoui)

`md2star-rs` est un spin-off du paquet Python [`md2star`](https://github.com/warith-harchaoui/md2star).
L'original est une fine surcouche (excellente) au-dessus de **Pandoc** ; celui-ci vise le
même but — du Markdown en entrée, un `.docx` fidèle en sortie — mais entièrement en Rust :

```text
Markdown ──▶ lecteur (pulldown-cmark → AST) ──▶ écrivain (AST → docx-rs) ──▶ .docx
```

## Pourquoi un spin-off Rust ?

`md2star` est fin *parce que Pandoc est épais* : les fonctionnalités vivent dans Pandoc, et
Pandoc est un binaire Haskell de ~100 Mo qu'il faut installer. C'est acceptable sur un poste
de travail ou en CI, mais cela exclut les téléphones, les machines verrouillées et le WASM.
`md2star-rs` échange l'étendue de Pandoc contre un **binaire unique autonome** et la
maîtrise totale de l'OOXML produit — d'où l'absence de la chirurgie `styles.xml`
(`postprocess.py`) de l'original.

## Installation

```bash
# Depuis les sources (nécessite Rust — https://rustup.rs)
cargo install --path .
# ou un binaire release
cargo build --release   # → target/release/md2docx
```

Voir [`scripts/brew.sh`](scripts/brew.sh) pour une installation de Rust via Homebrew
(macOS/Linux).

## Utilisation

```bash
md2docx rapport.md                  # → rapport.docx (à côté de l'entrée)
md2docx rapport.md -o out/final.docx
```

En bibliothèque :

```rust
use std::path::Path;
md2star_rs::markdown_to_docx_file("# Titre\n\nBonjour.", Path::new("out.docx")).unwrap();
```

Plus d'exemples dans [`EXAMPLES.md`](EXAMPLES.md).

## Périmètre et compromis face à Pandoc

Première version ciblée, pas un remplaçant de Pandoc. **Pas encore** ici, chacun étant une
suite propre et non une refonte : bibliographie/citations (pas de moteur CSL Rust stable),
math → OMML, vraies notes de bas de page Word, numérotation native des listes, hyperliens,
images intégrées, héritage de style `--reference-doc`, et PPTX. Pour tout cela, gardez
[`md2star`](https://github.com/warith-harchaoui/md2star) comme voie « fidélité maximale ».

## Licence

Apache-2.0 — voir [`LICENSE`](LICENSE).
