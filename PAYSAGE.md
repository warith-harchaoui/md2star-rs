# Paysage

🇫🇷 Français · [🇬🇧 LANDSCAPE.md](https://github.com/warith-harchaoui/md2star-rs/blob/main/LANDSCAPE.md)

Crates voisins puis concurrents dans l'espace « Markdown vers documents Office, en Rust », comparés à `md2star-rs`. Les notes
vont de ⭐ (1) à ⭐⭐⭐⭐⭐ (5), évaluées sur la tâche visée par `md2star-rs` :
du Markdown en entrée, un document Office fidèle en sortie, depuis un binaire statique unique sans Pandoc ni sous-processus. Un crate optimisé pour un tout autre usage n'est pas pénalisé : la note
reflète seulement l'adéquation à *ce* créneau.

## En un coup d'œil

<!-- TABLE:START -->
| Markdown vers Office en Rust | Entrée Markdown | Sortie DOCX | Sortie PPTX | Aucun binaire externe | Modèle / styles de référence | CLI prête à l'emploi | Sortie déterministe | Sortie PDF |
| --- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| **md2star-rs** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐ |
| docx-rs | ⭐ | ⭐⭐⭐⭐⭐ | ⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐ | ⭐⭐⭐⭐ | ⭐ |
| rdocx | ⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ |
| rpptx | ⭐ | ⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐ | ⭐⭐⭐ | ⭐⭐⭐ |
| pptxcraft | ⭐ | ⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐ | ⭐ | ⭐⭐⭐ | ⭐ |
| undoc | ⭐ | ⭐ | ⭐ | ⭐⭐⭐⭐⭐ | ⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐ |
| Pandoc | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ |
<!-- TABLE:END -->

## Carte de positionnement

<!-- FIGURE:START -->
Représentation 2D du tableau ci-dessus.

![Carte de positionnement](https://raw.githubusercontent.com/warith-harchaoui/md2star-rs/main/assets/paysage.png)

La carte est un résumé en 2D des 8 critères : à lire comme une forme, pas comme un classement. « md2star-rs » se situe dans le coin en haut à droite. Les axes se lisent **Horizontal : Simplicité ↔ Efficacité** et **Vertical : Précision ↔ Autonomie**.

L'axe horizontal demande si un projet prend du Markdown puis se livre comme un outil utilisable — l'entrée Markdown, un modèle de référence avec une CLI prête à l'emploi en sont les trois poids les plus forts. L'axe vertical arbitre entre déterminisme avec autonomie d'un côté, sortie PDF de l'autre, laquelle pèse négativement.

Deux projets seulement occupent la moitié droite : `md2star-rs` puis Pandoc. Tout le reste de cette carte est une bibliothèque qu'il faudrait piloter soi-même. Voilà la forme honnête de la comparaison — le vrai duel se joue face à Pandoc et `docx-rs`, `rpptx` puis `pptxcraft` sont des composants plutôt que des rivaux. Ce crate dépend des deux premiers.

La distance verticale entre `md2star-rs` en haut puis Pandoc en bas est un seul arbitrage, énoncé deux fois : Pandoc écrit des PDF mais doit être installé ; ce crate est un binaire statique déterministe qui n'écrit aucun PDF. `rdocx` est la ligne à surveiller — la plus proche du centre à l'horizontale, dans la moitié basse, parce que c'est une bibliothèque qui rend malgré tout des PDF.
<!-- FIGURE:END -->

## D'où viennent les affirmations

Versions puis dates de publication lues sur l'API crates.io le 29 octobre 2026 :

| Crate | Version | Dernière publication |
|---|---|---|
| `docx-rs` | 0.4.22 | 2026-07-21 |
| `rdocx` | 0.16.4 | 2026-10-28 |
| `rpptx` | 0.14.4 | 2026-10-28 |
| `pptxcraft` | 0.1.0 | 2026-10-21 |
| `undoc` | 0.27.1 | 2026-10-24 |

Les notes de `md2star-rs` lui-même proviennent de ce dépôt en version 0.4.4.

- **Pandoc figure dans le tableau bien qu'il ne soit pas un crate Rust.** C'est ce contre quoi ce crate se définit — le `md2star` Python l'enveloppe — donc l'omettre flatterait la comparaison. Il obtient 1 sur « Aucun binaire externe » pour la raison même qui justifie l'existence de ce crate, puis 5 sur « CLI prête à l'emploi » parce qu'il en est une : le problème de Pandoc n'a jamais été son interface.

- **L'unique 1 de la ligne de ce crate porte sur le PDF**, qui est le manque honnête face à l'original adossé à Pandoc : `md2star-rs` écrit du `.docx` puis du `.pptx`, pas du `.pdf`. `rdocx` y obtient 5 : c'est le crate à choisir quand le livrable est un PDF.

- **`docx-rs` puis `rpptx` obtiennent 1 sur l'entrée Markdown** parce qu'aucun des deux ne lit le Markdown : ce sont les écrivains OOXML. Ce crate en dépend plutôt que de les concurrencer ; la comparaison porte sur ce qu'un appelant obtient de bout en bout, non sur la qualité des bibliothèques.

- **`undoc` va dans l'autre sens** : il extrait du Markdown *depuis* des documents Office. Il est au tableau parce que c'est le crate le plus souvent confondu avec celui-ci dans une recherche, non parce qu'il le recouvre.

Laissé délibérément comme opinion plutôt que sourcé : les colonnes de style
*ergonomie*, puis l'ordre des lignes sous la référence.

## Idées à reprendre

Étudié plutôt que deviné : ce que chaque voisin fait mieux, puis ce que ce crate devrait changer pour l'égaler. Rien ici n'est un engagement : c'est la liste de lecture dont part la version suivante.

- **`rdocx` — produire un PDF depuis l'intérieur du crate** — L'unique 1 de ce crate est la sortie PDF et `rdocx` y obtient 5 tout en restant une bibliothèque Rust : il rend des PDF au pixel près puis exporte en HTML comme en Markdown. L'écart avec le `md2star` Python adossé à Pandoc est donc comblable sans réintroduire Pandoc, ce qui était toute la raison d'être de ce crate.

- **`undoc` — le sens inverse, d'Office vers Markdown** — Le `md2star` Python dispose de `twin`, qui relit un document fini vers du Markdown éditable. Le crate Rust n'a aucun sens inverse. `undoc` extrait DOCX, XLSX puis PPTX vers Markdown, texte brut avec JSON et montre la forme qu'un tel lecteur prendrait.

- **`pptxcraft` — graphiques, tableaux, images puis SVG dans une présentation** — L'écrivain PPTX d'ici transforme chaque titre `#` en diapositive avec ses puces. `pptxcraft` émet des graphiques, des tableaux, des images puis des dessins SVG : c'est la suite que réclame quiconque produit une vraie présentation.

- **`rpptx` — lire puis re-rendre une présentation existante** — Relire une présentation est ce dont une surface d'audit ou de lint a besoin et le côté Python dispose déjà de `md2star audit` pour exactement cela. `rpptx` sait lire, modifier puis rendre des présentations compatibles PowerPoint.
