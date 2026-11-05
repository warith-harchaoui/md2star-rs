//! Citations and the bibliography in DOCX, compared with Pandoc's citeproc output for the
//! same fixtures.

use std::io::{Cursor, Read};
use std::path::PathBuf;

use md2star_rs::{convert, Format, Options, Rendered};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn options() -> Options {
    let mut o = Options::default();
    o.base_dir = Some(fixtures());
    o
}

fn document(out: &Rendered) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(&out.bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

/// The visible text of each paragraph.
fn paragraphs(xml: &str) -> Vec<String> {
    xml.split("<w:p>")
        .skip(1)
        .map(|p| {
            let mut text = String::new();
            let mut rest = p;
            while let Some(at) = rest.find("<w:t xml:space=\"preserve\">") {
                rest = &rest[at + "<w:t xml:space=\"preserve\">".len()..];
                let end = rest.find("</w:t>").unwrap();
                text.push_str(&rest[..end]);
                rest = &rest[end..];
            }
            text.replace("&amp;", "&")
        })
        .filter(|t| !t.is_empty())
        .collect()
}

#[test]
fn the_bibliography_fixture_matches_pandoc() {
    let md = std::fs::read_to_string(fixtures().join("bibliography.md")).unwrap();
    let mut o = options();
    o.bibliography = vec![fixtures().join("refs.bib")];
    let out = convert(&md, Format::Docx, &o).unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    let xml = document(&out);
    let text = paragraphs(&xml);
    // Pandoc 3.6 --citeproc on the same file (chicago-author-date, its default).
    assert!(
        text.iter()
            .any(|p| p.contains("causal-inference framework of Pearl (2009) throughout")),
        "{text:#?}"
    );
    assert!(
        text.iter()
            .any(|p| p.contains("foundational treatment in (Pearl 2000).")),
        "{text:#?}"
    );
    let n = text.len();
    assert_eq!(text[n - 3], "Bibliography", "md2star's default heading");
    assert_eq!(
        &text[n - 2..],
        &[
            "Pearl, Judea. 2000. Causality: Models, Reasoning, and Inference. 1st ed. Cambridge University Press.",
            "———. 2009. Causality: Models, Reasoning, and Inference. 2nd ed. Cambridge University Press.",
        ]
    );
}

#[test]
fn entries_are_bookmarked_in_the_bibliography_style() {
    let mut o = options();
    o.bibliography = vec![fixtures().join("refs.bib")];
    let out = convert("See [@pearl2009].\n", Format::Docx, &o).unwrap();
    let xml = document(&out);
    assert!(xml.contains("<w:pStyle w:val=\"Heading1\"/></w:pPr><w:bookmarkStart w:id=\"1\" w:name=\"bibliography\"/>"));
    assert!(xml.contains("<w:bookmarkStart w:id=\"2\" w:name=\"refs\"/>"));
    assert!(xml.contains("<w:bookmarkStart w:id=\"3\" w:name=\"ref-pearl2009\"/><w:p><w:pPr><w:pStyle w:val=\"Bibliography\"/>"));
    // The title in italics.
    assert!(xml.contains("<w:i/><w:iCs/></w:rPr><w:t xml:space=\"preserve\">Causality: Models, Reasoning, and Inference</w:t>"));
}

#[test]
fn the_numeric_style_is_chosen_by_name() {
    let mut o = options();
    o.bibliography = vec![fixtures().join("refs.bib")];
    o.citation_style = Some("numeric".into());
    let out = convert(
        "A [@pearl2009, p. 3], B [@pearl2000; @pearl2009].\n",
        Format::Docx,
        &o,
    )
    .unwrap();
    let text = paragraphs(&document(&out));
    assert_eq!(text[0], "A [1, p. 3], B [1], [2].");
    assert!(text.iter().any(|p| p == "[1] J. Pearl, Causality: Models, reasoning, and inference, 2nd ed. Cambridge University Press, 2009."));
    // `csl: ieee.csl` in the front matter picks the same style.
    let out = convert(
        "---\ncsl: styles/ieee.csl\nbibliography: refs.bib\n---\n[@pearl2000]\n",
        Format::Docx,
        &options(),
    )
    .unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    assert_eq!(paragraphs(&document(&out))[0], "[1]");
}

#[test]
fn an_unknown_style_falls_back_with_a_warning() {
    let mut o = options();
    o.bibliography = vec![fixtures().join("refs.bib")];
    o.citation_style = Some("chicago-fullnote-bibliography.csl".into());
    let out = convert("[@pearl2000]\n", Format::Docx, &o).unwrap();
    assert!(out
        .warnings
        .iter()
        .any(|w| w.code == "citation-style-unknown"));
    assert_eq!(paragraphs(&document(&out))[0], "(Pearl 2000)");
}

#[test]
fn an_unknown_key_is_bold_as_written_with_a_warning() {
    let mut o = options();
    o.bibliography = vec![fixtures().join("refs.bib")];
    let out = convert("See [@nobody; @pearl2000].\n", Format::Docx, &o).unwrap();
    assert_eq!(out.warnings.len(), 1);
    assert_eq!(out.warnings[0].code, "citation-not-found");
    let xml = document(&out);
    assert!(xml.contains(
        "<w:rPr><w:b/><w:bCs/></w:rPr><w:t xml:space=\"preserve\">[@nobody; @pearl2000]</w:t>"
    ));
}

#[test]
fn the_heading_follows_options_then_front_matter_then_language() {
    let bib = fixtures().join("refs.bib");
    let run = |md: &str, name: Option<&str>| {
        let mut o = options();
        o.bibliography = vec![bib.clone()];
        o.bibliography_name = name.map(String::from);
        let out = convert(md, Format::Docx, &o).unwrap();
        paragraphs(&document(&out))
    };
    assert!(run("[@pearl2000]\n", Some("Sources")).contains(&"Sources".to_string()));
    assert!(run(
        "---\nreference-section-title: Works cited\n---\n[@pearl2000]\n",
        None
    )
    .contains(&"Works cited".to_string()));
    assert!(
        run("---\nlang: fr-FR\n---\n[@pearl2000]\n", None).contains(&"Bibliographie".to_string())
    );
}

#[test]
fn a_missing_bibliography_is_a_warning_and_citations_stay_as_written() {
    let mut o = options();
    o.bibliography = vec![fixtures().join("nowhere.bib")];
    let out = convert("See [@pearl2000].\n", Format::Docx, &o).unwrap();
    assert!(out
        .warnings
        .iter()
        .any(|w| w.code == "bibliography-unreadable"));
    assert_eq!(paragraphs(&document(&out))[0], "See [@pearl2000].");
}

#[test]
fn without_a_bibliography_citations_are_left_as_written() {
    let out = convert(
        "See [@pearl2000, p. 3] and @pearl2009.\n",
        Format::Docx,
        &Options::default(),
    )
    .unwrap();
    assert!(out.warnings.is_empty());
    assert_eq!(
        paragraphs(&document(&out))[0],
        "See [@pearl2000, p. 3] and @pearl2009."
    );
}

#[test]
fn the_large_fixture_bibliography_renders_every_entry() {
    let src = std::fs::read_to_string(fixtures().join("references.bib")).unwrap();
    let keys: Vec<String> = src
        .lines()
        .filter_map(|l| l.strip_prefix('@'))
        .filter_map(|l| l.split_once('{'))
        .map(|(_, k)| k.trim_end_matches(',').to_string())
        .collect();
    let md: String = keys.iter().map(|k| format!("[@{k}]\n\n")).collect();
    let mut o = options();
    o.bibliography = vec![fixtures().join("references.bib")];
    let out = convert(&md, Format::Docx, &o).unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    let xml = document(&out);
    assert_eq!(
        xml.matches("<w:pStyle w:val=\"Bibliography\"/>").count(),
        keys.len()
    );
    let text = paragraphs(&xml);
    // Lines Pandoc 3.6 writes identically (chicago-author-date).
    for line in [
        "(Golub and Loan 2013)",
        "(Bouveyron et al. 2021)",
        "(Goodfellow, Bengio, and Courville 2016)",
        "Golub, Gene H., and Charles F. Van Loan. 2013. Matrix Computations, 4th Edition. https://www.press.jhu.edu/books/title/10678/matrix-computations.",
        "Prado, Marcos Lopez de. 2018. Advances in Financial Machine Learning. https://www.amazon.com/Advances-Financial-Machine-Learning-Marcos/dp/1119482089.",
        "Cook, Alexis, and Kaggle. 2017. “Kaggle Learn.” https://www.kaggle.com/learn.",
        "Goldberg, Yoav. 2017. Neural Network Methods in Natural Language Processing. https://u.cs.biu.ac.il/~yogo/nnlp.pdf.",
    ] {
        assert!(text.iter().any(|t| t == line), "missing: {line}");
    }
}

/// Pandoc 3.6.1 `--citeproc` on every entry of `tests/fixtures/references.bib`, cited
/// once each in its own paragraph, rendered to plain text (`-t plain --wrap=none`):
/// chicago-author-date (Pandoc's default) and `--csl ieee.csl` (the official style). The
/// IEEE run also cites `refs.bib` first, through the sentence below.
const PANDOC_CHICAGO: &str = r#"(Pearl 2009)
(Lechelle 2024)
(Marr 1982)
(Woodlock 2010)
(Golub and Loan 2013)
(Bouveyron et al. 2021)
(Azencott 2022)
(Bach 2024)
(Warmerdam 2024)
(Vandenbussche 2023)
(Goodfellow, Bengio, and Courville 2016)
(Cook and Kaggle 2017)
(Kilcher 2017)
(Larochelle 2013)
(Hastie, Tibshirani, and Friedman 2009)
(Bishop 2006)
(Murphy 2012)
(Barber 2012)
(Szeliski 2022)
(Forsyth and Ponce 2011)
(Hartley and Zisserman 2004)
(Goldberg 2017)
(C. Manning and Schütze 1999)
(Mallat 2008)
(MacKay 2003)
(Prado 2018)
(C. D. Manning, Raghavan, and Schütze 2008)
(Sutton and Barto 2018)
(Cormen et al. 2022)
(Boyd and Vandenberghe 2004)
(Bonnans et al. 2006)
(Press et al. 2007)
(Peyré and Cuturi 2020)
(Howard and Gugger 2017)
(Dehaene, LeCun, and Girardon 2018)
(Gerbaud and Gerbaud 2024)
Azencott, Chloé-Agathe. 2022. Introduction Au Machine Learning. https://cazencott.info/index.php/pages/Introduction-au-Machine-Learning.
Bach, Francis. 2024. Learning Theory from First Principles. https://francisbach.com/my-book-is-out/.
Barber, David. 2012. Bayesian Reasoning and Machine Learning. https://tinyurl.com/brml-barber.
Bishop, Christopher M. 2006. Pattern Recognition and Machine Learning. https://tinyurl.com/bishop-prml.
Bonnans, J. Frédéric, J. Charles Gilbert, Claude Lemaréchal, and Claudia A. Sagastizábal. 2006. Numerical Optimization, 2nd Edition. https://tinyurl.com/bonnans-numerical-optimization.
Bouveyron, Charles, Gilles Celeux, T. Brendan Murphy, and Adrian E. Raftery. 2021. Model-Based Clustering and Classification for Data Science. https://math.univ-cotedazur.fr/~cbouveyr/MBCbook/.
Boyd, Stephen, and Lieven Vandenberghe. 2004. Convex Optimization. https://tinyurl.com/cvxboyd.
Cook, Alexis, and Kaggle. 2017. “Kaggle Learn.” https://www.kaggle.com/learn.
Cormen, Thomas H., Charles E. Leiserson, Ronald L. Rivest, and Clifford Stein. 2022. Introduction to Algorithms. https://mitpress.mit.edu/9780262046305/introduction-to-algorithms/.
Dehaene, Stanislas, Yann LeCun, and Jacques Girardon. 2018. La Plus Belle Histoire de l’intelligence : Des Origines Aux Neurones Artificiels. https://www.lisez.com/livres/la-plus-belle-histoire-de-lintelligence/9782221221105.
Forsyth, David, and Jean Ponce. 2011. Computer Vision: A Modern Approach, 2nd Edition. https://tinyurl.com/forsyth-ponce-cv.
Gerbaud, Antoine, and Thomas Gerbaud. 2024. “Algonaute – Une Sélection Personnelle d’ouvrages Techniques (Ou Presque).” https://algonaute.fr/fr/livres.html.
Goldberg, Yoav. 2017. Neural Network Methods in Natural Language Processing. https://u.cs.biu.ac.il/~yogo/nnlp.pdf.
Golub, Gene H., and Charles F. Van Loan. 2013. Matrix Computations, 4th Edition. https://www.press.jhu.edu/books/title/10678/matrix-computations.
Goodfellow, Ian, Yoshua Bengio, and Aaron Courville. 2016. Deep Learning. https://www.deeplearningbook.org/.
Hartley, Richard, and Andrew Zisserman. 2004. Multiple View Geometry in Computer Vision. https://tinyurl.com/hartley-zisserman.
Hastie, Trevor, Robert Tibshirani, and Jerome Friedman. 2009. The Elements of Statistical Learning, 2nd Edition. https://tinyurl.com/elstatlearning.
Howard, Jeremy, and Sylvain Gugger. 2017. “Fast.ai.” https://www.fast.ai.
Kilcher, Yannic. 2017. “Yannic Kilcher.” https://www.youtube.com/c/yannickilcher.
Larochelle, Hugo. 2013. “Hugo Larochelle.” https://www.youtube.com/@hugolarochelle.
Lechelle, Yann. 2024. Ouvertarisme. https://ouvertarisme.s3-website.fr-par.scw.cloud.
MacKay, David. 2003. Information Theory, Inference, and Learning Algorithms. https://tinyurl.com/mackay-information.
Mallat, Stéphane. 2008. A Wavelet Tour of Signal Processing, 3rd Edition. https://tinyurl.com/mallat-signal.
Manning, Chris, and Hinrich Schütze. 1999. Foundations of Statistical Natural Language Processing. https://tinyurl.com/manning-nlp.
Manning, Christopher D., Prabhakar Raghavan, and Hinrich Schütze. 2008. Introduction to Information Retrieval. https://tinyurl.com/manning-information-retireval.
Marr, David C. 1982. Vision: A Computational Investigation into the Human Representation and Processing of Visual Information. https://mitpress.mit.edu/9780262514620/vision/.
Murphy, Kevin. 2012. Machine Learning: A Probabilistic Perspective. https://tinyurl.com/murphy-ml.
Pearl, Judea. 2009. Causality: Models, Reasoning, and Inference, 2nd Edition. https://bayes.cs.ucla.edu/BOOK-2K/.
Peyré, Gabriel, and Marco Cuturi. 2020. Computational Optimal Transport. https://tinyurl.com/peyre-cuturi-ot/.
Prado, Marcos Lopez de. 2018. Advances in Financial Machine Learning. https://www.amazon.com/Advances-Financial-Machine-Learning-Marcos/dp/1119482089.
Press, William H., Saul A. Teukolsky, William T. Vetterling, and Brian P. Flannery. 2007. Numerical Recipes, 3rd Edition. https://numerical.recipes/.
Sutton, Richard S., and Andrew G. Barto. 2018. Reinforcement Learning, 2nd Edition. https://tinyurl.com/sutton-barto-rl.
Szeliski, Richard. 2022. Computer Vision: Algorithms and Applications, 2nd Edition. https://tinyurl.com/good-cv.
Vandenbussche, Vincent. 2023. The Regularization Cookbook. https://www.amazon.com/Regularization-Cookbook-practical-recipes-robustness-ebook/dp/B0BRSSG5Z6.
Warmerdam, Vincent D. 2024. “:probabl.” https://www.youtube.com/@probabl_ai.
Woodlock, Don. 2010. “Don Woodlock.” https://www.youtube.com/dwoodlock."#;
const PANDOC_IEEE: &str = r#"One [1, p. 3], two [1], [2], in-text [2] says, suppress [1].
[3]
[4]
[5]
[6]
[7]
[8]
[9]
[10]
[11]
[12]
[13]
[14]
[15]
[16]
[17]
[18]
[19]
[20]
[21]
[22]
[23]
[24]
[25]
[26]
[27]
[28]
[29]
[30]
[31]
[32]
[33]
[34]
[35]
[36]
[37]
[38]
[1] J. Pearl, Causality: Models, reasoning, and inference, 2nd ed. Cambridge University Press, 2009.
[2] J. Pearl, Causality: Models, reasoning, and inference, 1st ed. Cambridge University Press, 2000.
[3] J. Pearl, Causality: Models, reasoning, and inference, 2nd edition. 2009. Accessed: Feb. 21, 2026. [Online]. Available: https://bayes.cs.ucla.edu/BOOK-2K/
[4] Y. Lechelle, Ouvertarisme. 2024. Accessed: Feb. 21, 2026. [Online]. Available: https://ouvertarisme.s3-website.fr-par.scw.cloud
[5] D. C. Marr, Vision: A computational investigation into the human representation and processing of visual information. 1982. Accessed: Feb. 21, 2026. [Online]. Available: https://mitpress.mit.edu/9780262514620/vision/
[6] D. Woodlock, “Don woodlock.” 2010. Accessed: Feb. 21, 2026. [Online]. Available: https://www.youtube.com/dwoodlock
[7] G. H. Golub and C. F. V. Loan, Matrix computations, 4th edition. 2013. Accessed: Feb. 21, 2026. [Online]. Available: https://www.press.jhu.edu/books/title/10678/matrix-computations
[8] C. Bouveyron, G. Celeux, T. B. Murphy, and A. E. Raftery, Model-based clustering and classification for data science. 2021. Accessed: Feb. 21, 2026. [Online]. Available: https://math.univ-cotedazur.fr/~cbouveyr/MBCbook/
[9] C.-A. Azencott, Introduction au machine learning. 2022. Accessed: Feb. 21, 2026. [Online]. Available: https://cazencott.info/index.php/pages/Introduction-au-Machine-Learning
[10] F. Bach, Learning theory from first principles. 2024. Accessed: Feb. 21, 2026. [Online]. Available: https://francisbach.com/my-book-is-out/
[11] V. D. Warmerdam, “:probabl.” 2024. Accessed: Feb. 21, 2026. [Online]. Available: https://www.youtube.com/@probabl_ai
[12] V. Vandenbussche, The regularization cookbook. 2023. Accessed: Feb. 21, 2026. [Online]. Available: https://www.amazon.com/Regularization-Cookbook-practical-recipes-robustness-ebook/dp/B0BRSSG5Z6
[13] I. Goodfellow, Y. Bengio, and A. Courville, Deep learning. 2016. Accessed: Feb. 21, 2026. [Online]. Available: https://www.deeplearningbook.org/
[14] A. Cook and Kaggle, “Kaggle learn.” 2017. Accessed: Feb. 21, 2026. [Online]. Available: https://www.kaggle.com/learn
[15] Y. Kilcher, “Yannic kilcher.” 2017. Accessed: Feb. 21, 2026. [Online]. Available: https://www.youtube.com/c/yannickilcher
[16] H. Larochelle, “Hugo larochelle.” 2013. Accessed: Feb. 21, 2026. [Online]. Available: https://www.youtube.com/@hugolarochelle
[17] T. Hastie, R. Tibshirani, and J. Friedman, The elements of statistical learning, 2nd edition. 2009. Accessed: Feb. 21, 2026. [Online]. Available: https://tinyurl.com/elstatlearning
[18] C. M. Bishop, Pattern recognition and machine learning. 2006. Accessed: Feb. 21, 2026. [Online]. Available: https://tinyurl.com/bishop-prml
[19] K. Murphy, Machine learning: A probabilistic perspective. 2012. Accessed: Feb. 21, 2026. [Online]. Available: https://tinyurl.com/murphy-ml
[20] D. Barber, Bayesian reasoning and machine learning. 2012. Accessed: Feb. 21, 2026. [Online]. Available: https://tinyurl.com/brml-barber
[21] R. Szeliski, Computer vision: Algorithms and applications, 2nd edition. 2022. Accessed: Feb. 21, 2026. [Online]. Available: https://tinyurl.com/good-cv
[22] D. Forsyth and J. Ponce, Computer vision: A modern approach, 2nd edition. 2011. Accessed: Feb. 21, 2026. [Online]. Available: https://tinyurl.com/forsyth-ponce-cv
[23] R. Hartley and A. Zisserman, Multiple view geometry in computer vision. 2004. Accessed: Feb. 21, 2026. [Online]. Available: https://tinyurl.com/hartley-zisserman
[24] Y. Goldberg, Neural network methods in natural language processing. 2017. Accessed: Feb. 21, 2026. [Online]. Available: https://u.cs.biu.ac.il/~yogo/nnlp.pdf
[25] C. Manning and H. Schütze, Foundations of statistical natural language processing. 1999. Accessed: Feb. 21, 2026. [Online]. Available: https://tinyurl.com/manning-nlp
[26] S. Mallat, A wavelet tour of signal processing, 3rd edition. 2008. Accessed: Feb. 21, 2026. [Online]. Available: https://tinyurl.com/mallat-signal
[27] D. MacKay, Information theory, inference, and learning algorithms. 2003. Accessed: Feb. 21, 2026. [Online]. Available: https://tinyurl.com/mackay-information
[28] M. L. de Prado, Advances in financial machine learning. 2018. Accessed: Feb. 21, 2026. [Online]. Available: https://www.amazon.com/Advances-Financial-Machine-Learning-Marcos/dp/1119482089
[29] C. D. Manning, P. Raghavan, and H. Schütze, Introduction to information retrieval. 2008. Accessed: Feb. 21, 2026. [Online]. Available: https://tinyurl.com/manning-information-retireval
[30] R. S. Sutton and A. G. Barto, Reinforcement learning, 2nd edition. 2018. Accessed: Feb. 21, 2026. [Online]. Available: https://tinyurl.com/sutton-barto-rl
[31] T. H. Cormen, C. E. Leiserson, R. L. Rivest, and C. Stein, Introduction to algorithms. 2022. Accessed: Feb. 21, 2026. [Online]. Available: https://mitpress.mit.edu/9780262046305/introduction-to-algorithms/
[32] S. Boyd and L. Vandenberghe, Convex optimization. 2004. Accessed: Feb. 21, 2026. [Online]. Available: https://tinyurl.com/cvxboyd
[33] J. F. Bonnans, J. C. Gilbert, C. Lemaréchal, and C. A. Sagastizábal, Numerical optimization, 2nd edition. 2006. Accessed: Feb. 21, 2026. [Online]. Available: https://tinyurl.com/bonnans-numerical-optimization
[34] W. H. Press, S. A. Teukolsky, W. T. Vetterling, and B. P. Flannery, Numerical recipes, 3rd edition. 2007. Accessed: Feb. 21, 2026. [Online]. Available: https://numerical.recipes/
[35] G. Peyré and M. Cuturi, Computational optimal transport. 2020. Accessed: Feb. 21, 2026. [Online]. Available: https://tinyurl.com/peyre-cuturi-ot/
[36] J. Howard and S. Gugger, “Fast.ai.” 2017. Accessed: Feb. 21, 2026. [Online]. Available: https://www.fast.ai
[37] S. Dehaene, Y. LeCun, and J. Girardon, La plus belle histoire de l’intelligence : Des origines aux neurones artificiels. 2018. Accessed: Feb. 21, 2026. [Online]. Available: https://www.lisez.com/livres/la-plus-belle-histoire-de-lintelligence/9782221221105
[38] A. Gerbaud and T. Gerbaud, “Algonaute – une sélection personnelle d’ouvrages techniques (ou presque).” 2024. Accessed: Feb. 21, 2026. [Online]. Available: https://algonaute.fr/fr/livres.html"#;
const IEEE_LEAD: &str = r#"One [@pearl2009, p. 3], two [@pearl2000; @pearl2009], in-text @pearl2000 says, suppress [-@pearl2009]."#;

fn every_key() -> Vec<String> {
    std::fs::read_to_string(fixtures().join("references.bib"))
        .unwrap()
        .lines()
        .filter_map(|l| l.strip_prefix('@'))
        .filter_map(|l| l.split_once('{'))
        .map(|(_, k)| k.trim_end_matches(',').to_string())
        .collect()
}

#[test]
fn every_entry_renders_word_for_word_as_pandoc_chicago() {
    let md: String = every_key().iter().map(|k| format!("[@{k}]\n\n")).collect();
    let mut o = options();
    o.bibliography = vec![fixtures().join("references.bib")];
    o.bibliography_name = Some(String::new());
    let out = convert(&md, Format::Docx, &o).unwrap();
    assert_eq!(paragraphs(&document(&out)).join("\n"), PANDOC_CHICAGO);
}

#[test]
fn every_entry_renders_word_for_word_as_pandoc_ieee() {
    let md: String = std::iter::once(format!("{IEEE_LEAD}\n\n"))
        .chain(every_key().iter().map(|k| format!("[@{k}]\n\n")))
        .collect();
    let mut o = options();
    o.bibliography = vec![
        fixtures().join("references.bib"),
        fixtures().join("refs.bib"),
    ];
    o.bibliography_name = Some(String::new());
    o.citation_style = Some("ieee.csl".into());
    let out = convert(&md, Format::Docx, &o).unwrap();
    assert_eq!(paragraphs(&document(&out)).join("\n"), PANDOC_IEEE);
}
