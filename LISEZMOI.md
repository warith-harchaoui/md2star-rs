# md2star-rs

[🇫🇷 LISEZMOI](LISEZMOI.md) · [🇬🇧 README](README.md)

[![crates.io](https://img.shields.io/crates/v/md2star-rs.svg)](https://crates.io/crates/md2star-rs)

**Du Markdown vers Word, PowerPoint et PDF, puis de Word ou PowerPoint vers le Markdown, en
Rust pur. Ni Pandoc, ni LibreOffice, ni LaTeX, ni sous-processus : la même bibliothèque
tourne sur un serveur, un portable, un iPhone ou un téléphone Android.**

Par [Warith HARCHAOUI](https://linkedin.com/in/warith-harchaoui)

`md2star-rs` est le portage multi-appareils du
[`md2star`](https://github.com/warith-harchaoui/md2star) Python. La version Python pilote
Pandoc, LibreOffice puis un modèle de langage ; celle-ci fait le même travail dans le
processus. Son cœur compile pour iOS comme pour Android, sa CLI tient en un seul binaire
statique.

```text
                                                  ┌──▶ docx ──▶ .docx
Markdown ──▶ lecteur ──▶ AST ──(citations BibTeX)─┼──▶ pptx ──▶ .pptx   (dispositions du modèle)
                                                  └──▶ pdf  ──▶ .pdf    (Typst, fonctionnalité `pdf`)

.docx / .pptx ──▶ twin ──▶ Markdown            .pptx ──▶ audit ──▶ constats
```

## Ce qui marche

| Sortie | Ce qu'elle fait | Vérifié par |
|---|---|---|
| **DOCX** (`md2star-rs docx`) | Le vocabulaire WordprocessingML de Pandoc, écrit directement : page de titre tirée du front matter, table des matières (`--toc`), titres munis de signets, numérotation native des listes, cases à cocher, tableaux avec alignement et légende, vraies notes de bas de page, vrais hyperliens et sauts `#id`, images intégrées (PNG, JPEG, GIF, BMP, TIFF, SVG avec son repli PNG) mises à la largeur de la colonne, **équations natives** (LaTeX → OMML), **citations et bibliographie** tirées d'un `.bib`, schémas Mermaid en images (fonctionnalité `mermaid`). Le `template.docx` de md2star sert de modèle par défaut ; `--reference-doc` garde toutes les parties d'un autre (styles, thème, numérotation, en-têtes, pieds de page, mise en page). | `tests/docx_*.rs`, `tests/convert.rs`, `tests/reference_doc.rs` ouvrent le zip pour lire le XML |
| **PPTX** (`md2star-rs pptx`) | Du PresentationML écrit sur les masques et dispositions du modèle (le `template.pptx` de md2star par défaut). Chaque diapositive remplit les espaces réservés d'une disposition choisie **de façon déterministe** : `{layout="Nom"}`, une classe de rôle (`.statement` `.bignumber` `.quote` `.recap` `.agenda` `.section` `.closing`), une forme reconnue, sinon un score d'ajustement mesuré avec les vraies chasses des polices du thème. `--min-pt N` ne pose jamais de texte sous N points : la diapositive passe sur une disposition plus vaste ou se coupe. Images, tableaux, code, deux colonnes, notes du présentateur, notes de bas de page, équations (OMML), liens. `--smart-layout` (fonctionnalité `llm`) laisse un modèle trancher les quasi-égalités, rien de plus. | `tests/pptx_*.rs`, `tests/audit_decks.rs` |
| **PDF** (`md2star-rs pdf`, fonctionnalité `pdf`) | Markdown → balisage Typst → PDF, compilé en mémoire avec des polices embarquées dans le binaire : le même fichier sur chaque machine à chaque exécution. Page de titre, titres, listes, tableaux, figures, notes de bas de page, liens, table des matières, citations, mathématiques natives (LaTeX → maths Typst). | `tests/pdf_render.rs` |
| **twin** (`md2star-rs twin`) | N'importe quel `.docx` ou `.pptx` ramené à un Markdown éditable, images extraites dans `<nom>_media/` : titres, listes avec leur numérotation, tableaux, liens, notes, code, citations en retrait, équations reconverties en LaTeX, notes du présentateur. Graphiques et SmartArt sont signalés, jamais inventés. | `tests/twin_*.rs` : allers-retours Markdown → DOCX/PPTX → Markdown, puis les sorties de md2star lui-même |
| **audit** (`md2star-rs audit`) | N'importe quel `.pptx` : `off-slide`, `overlap` (texte sur texte, image sur texte), `ghost-placeholder`, `illegible-type`, `under-min-pt`, `low-contrast` (ratio WCAG face au fond résolu, couleurs du thème comprises), `overrun`. Mêmes catégories, même JSON, mêmes codes de sortie que `md2star audit`. | `tests/audit_decks.rs` |
| **templates** (`md2star-rs templates`) | Liste ou exporte les modèles embarqués ; montre les dispositions puis les espaces réservés que lit le planificateur de diapositives dans n'importe quel `.pptx`. | `tests/templates_cli.rs` |

Chaque sortie est **identique octet pour octet d'une exécution à l'autre** (DOCX, PPTX
comme PDF). Aucun contenu ne fait échouer une conversion : ce qui ne peut pas se faire
devient un avertissement rendu à côté des octets.

## Multi-appareils, prouvé plutôt qu'affirmé

À chaque push, la CI lance `cargo check --lib` pour `aarch64-apple-ios` puis
`aarch64-linux-android`, trois fois par cible : nu (`--no-default-features`), avec `svg`,
puis avec `svg,pdf,mermaid`. `cargo check` demande la bibliothèque standard de la cible
mais aucun SDK, donc une machine Ubuntu suffit à le prouver. Le cœur n'a aucune dépendance
en C. Typst en apporte une, `psm`, qui assemble quelques lignes avec le compilateur C de la
cible (celui de Xcode ou du NDK dans une app).

Le même travail Ubuntu compile puis analyse `--all-features`. Aucune voie GPU n'a de sens
ici : la conversion, c'est du XML, de la mesure de texte et de la composition sur le
processeur, il n'y a donc aucune accélération à activer, sur Ubuntu ni ailleurs.

## Fonctionnalités Cargo et leur coût

Tailles : le binaire `md2star-rs` de release, débarrassé de ses symboles, macOS arm64,
Rust 1.95, mesuré pour cette version.

| Fonctionnalités | Ce qu'elles ajoutent | Binaire | MSRV |
|---|---|---|---|
| `cli` | les binaires `md2star-rs`, `md2docx`, `md2pptx` (`clap`) | 3,6 Mo | 1.88 |
| `cli`, `svg` (le défaut) | le repli PNG que Word exige sous une image SVG, rastérisé par `resvg` | 6,4 Mo | 1.88 |
| + `pdf` | le PDF dans le processus par Typst, avec ses polices (environ 10 Mo à elles seules) | 50,4 Mo | **1.92** |
| + `pdf`, `mermaid` | les schémas Mermaid dessinés en SVG (`mermaid-rs-renderer`) dans DOCX, PPTX puis PDF | 52,9 Mo | 1.92 |
| toutes | + `net` (télécharger les images `http(s)://`, toujours sur demande explicite) puis `llm` (`--smart-layout` par un point d'accès compatible OpenAI) | 54,7 Mo | 1.92 |

La MSRV de la crate est **1.88** ; seule la fonctionnalité `pdf` exige **1.92** (le
plancher de Typst 0.15). Les deux ont été vérifiées sur de vraies chaînes 1.88.0 et 1.92.0
pour cette version.

## Installation

```bash
cargo install md2star-rs                          # docx, pptx, twin, audit, templates
cargo install md2star-rs --features pdf,mermaid   # + PDF et schémas (Rust 1.92+)
```

Comme bibliothèque :

```toml
[dependencies]
md2star-rs = "0.5"
# le cœur seul (sans CLI), tel qu'une app iOS ou Android l'embarque :
# md2star-rs = { version = "0.5", default-features = false }
```

## Utilisation

```bash
md2star-rs docx rapport.md                          # → rapport.docx
md2star-rs docx rapport.md --bib refs.bib --toc     # citations, table des matières
md2star-rs docx rapport.md --reference-doc maison.docx
md2star-rs pptx expose.md --min-pt 24               # → expose.pptx, rien sous 24 pt
md2star-rs pdf rapport.md                           # → rapport.pdf (fonctionnalité `pdf`)
md2star-rs twin rapport.docx                        # → rapport.md + rapport_media/
md2star-rs audit expose.pptx --json                 # sortie 1 sur un défaut
md2star-rs templates layouts maison.pptx            # ce que voit le planificateur
```

Toutes les options sont dans
[`skills/md2star-rs/references/cli-reference.md`](skills/md2star-rs/references/cli-reference.md).

```rust
use md2star_rs::{convert, Format, Options};

let mut options = Options::default();
options.bibliography = vec!["refs.bib".into()];
let out = convert("# Titre\n\nComme on l'a vu [@pearl2009].", Format::Docx, &options)?;
std::fs::write("sortie.docx", &out.bytes)?;
for w in &out.warnings {
    eprintln!("{}: {}", w.code, w.message);
}

let twin = md2star_rs::twin::to_markdown(
    &std::fs::read("expose.pptx")?,
    md2star_rs::twin::SourceKind::Pptx,
    &Default::default(),
)?;
println!("{}", twin.markdown);
```

D'autres recettes dans [`EXAMPLES.md`](EXAMPLES.md).

## Comparaison mesurée avec md2star (Python)

| Point | Résultat | Comment |
|---|---|---|
| Citations | mot pour mot ce qu'écrit Pandoc 3.6.1 `--citeproc`, en Chicago auteur-date comme en IEEE, pour chaque entrée du `.bib` des fixtures | `tests/docx_citations.rs` |
| Équations (OMML) | 49 formules sur 60 ont exactement la structure qu'écrit Pandoc 3.6.1. Sur les 11 autres, 10 diffèrent à dessein (objets n-aires pour `\prod` `\oint` `\bigcup` `\iint`, `limLow` pour `\lim` `\max` `\operatorname*`, accolades en `groupChr`, pas de ligne vide en fin de matrice) ; `\big(` est posé en texte simple | un corpus de 62 formules (les tests du convertisseur puis les fixtures), comparées après retrait des propriétés et fusion des runs ; Pandoc en a refusé 2 |
| Dispositions des diapositives | sur les 10 diapositives indicées de `tests/fixtures/deck.md`, les dispositions que choisit md2star 3.12. md2star 3.13.1 s'accorde sur 9 puis pose un tableau seul sur « Two Content », laissant une zone vide | `tests/pptx_layout.rs`, relancé contre `md2pptx --smart-layout --no-visual-confirm` |
| Audit | les constats de `md2star audit --min-pt 30 --json` (md2star 3.13.1) sur les présentations produites par Pandoc, diapositive par diapositive, forme par forme | `tests/audit_decks.rs` |

## Hors périmètre : pourquoi

- **PDF → Markdown.** `twin` lit le `.docx` et le `.pptx`. Un PDF demande une analyse de
  mise en page puis de l'OCR, ce que rien en Rust pur ne fait bien sur un téléphone ; il
  est refusé avec cette raison.
- **CSL complet.** Deux styles sont intégrés, Chicago auteur-date puis IEEE numérique. Le
  *nom* d'un fichier `--csl` choisit l'un d'eux ; son contenu n'est jamais lu.
- **Mermaid d'une machine à l'autre.** `mermaid-rs-renderer` mesure le texte avec les
  polices de la machine : un schéma est stable d'une exécution à l'autre sur une machine,
  pas identique entre deux. Il garde une police en cache sous `~/.cache/mmdr`.
- **`--reference-slides`** (cloner les diapositives d'exemple d'un modèle) puis le **juge
  visuel** de md2star (un modèle de vision qui regarde les diapositives rendues). L'`audit`
  géométrique remplace le second ; aucun des deux ne tient sur un téléphone.
- **La GUI, le serveur HTTP, les outils MCP puis `doctor`** de la version Python : une
  bibliothèque multi-appareils n'en a pas l'usage.

## Trois binaires : lequel choisir

`cargo install md2star-rs` installe **`md2star-rs`**, `md2docx` puis `md2pptx`. Les deux
derniers portent les noms qu'installe le `md2star` Python : avec les deux sur une machine,
c'est l'ordre du `PATH` qui décide lequel tourne. Employez `md2star-rs docx` puis
`md2star-rs pptx` dans les scripts, les Dockerfiles comme en CI : ils ne peuvent entrer en
collision avec rien, leur sortie est identique octet pour octet.

## Skill pour agents

`skills/md2star-rs/` fait de cette crate une Claude Skill ainsi qu'une skill OpenCode
([`skills/README.md`](skills/README.md)). Son catalogue de déclencheurs,
[`TRIGGERS.md`](TRIGGERS.md), a pour miroir `references/triggers.md` ; `tests/skill.rs`
fait échouer la compilation s'ils divergent.

## Développement

```bash
scripts/install-hooks.sh   # une fois par clone : la barrière pre-push
```

Le hook pre-push lance `cargo fmt --check`, clippy avec les avertissements en erreurs
(défaut, sans fonctionnalité, toutes), les tests (défaut, sans fonctionnalité, toutes),
`cargo doc` avec les avertissements en erreurs, les vérifications iOS puis Android, enfin
`scripts/check-fresh-resolve.sh` avec puis sans fonctionnalités : une compilation sans
`Cargo.lock`, telle qu'un `cargo add` en aval l'obtient (md2star-rs 0.4.0 a déjà été publié
impossible à installer alors que toutes les compilations verrouillées passaient au vert).
La CI rejoue la partie légère sur un seul travail Ubuntu. Aucun fichier suivi ne peut
contenir le chemin d'une machine : `tests/no_personal_paths.rs` y veille.

## Licence

Apache-2.0, voir [`LICENSE`](LICENSE).
