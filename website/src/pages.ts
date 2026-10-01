/** The GitHub repository the site is built from. */
export const REPOSITORY_URL = "https://github.com/HerringtonDarkholme/hd-lang";

/** One Markdown source rendered as one site page. */
export interface PageSource {
  /** Repository-relative Markdown path. */
  readonly source: string;
  /** Output path relative to the site root. */
  readonly output: string;
  /** Sidebar label. */
  readonly navTitle: string;
  /** Sidebar section. */
  readonly section: string;
  /**
   * Set for a page that no Markdown file holds: the build generates its
   * Markdown, and renders it as if it were `source`.
   */
  readonly generated?: "glossary";
}

export interface NavSection {
  readonly title: string;
  readonly pages: readonly PageSource[];
}

const SPEC_CHAPTERS: readonly [file: string, title: string][] = [
  ["01-lexical-structure", "Lexical Structure"],
  ["02-grammar", "Grammar"],
  ["03-names-and-scopes", "Names and Scopes"],
  ["04-type-system", "Type System"],
  ["05-expressions", "Expressions"],
  ["06-control-flow", "Control Flow"],
  ["07-functions", "Functions"],
  ["08-data-and-enums", "Data Types and Enums"],
  ["09-traits", "Traits"],
  ["10-modules", "Modules"],
  ["11-requirements-and-suspension", "Requirements and Suspension"],
  ["12-variadic-generics", "Variadic Generics"],
  ["13-gadts", "GADTs"],
  ["14-annotations", "Annotations"],
];

/** The stdlib chapters in spec/std/, one per std module, in reading order. */
const STD_CHAPTERS: readonly [module: string, title: string][] = [
  ["format", "Format"],
  ["iter", "Iterators"],
  ["task", "Task"],
  ["testing", "Testing"],
  ["text", "Text"],
  ["time", "Time"],
];

const page = (source: string, output: string, navTitle: string, section: string): PageSource => ({
  source,
  output,
  navTitle,
  section,
});

/** Every rendered page, in reading order. */
export const PAGES: readonly PageSource[] = [
  page("README.md", "index.html", "Home", "Start"),
  page(
    "guide/LEARN_IN_10_MINUTES.md",
    "guide/learn-in-10-minutes.html",
    "Learn in 10 Minutes",
    "Start",
  ),
  page("guide/README.md", "guide/index.html", "Guide", "Guide"),
  page("guide/OVERVIEW.md", "guide/overview.html", "Language Overview", "Guide"),
  page("guide/LANGUAGE_TOUR.md", "guide/language-tour.html", "Language Tour", "Guide"),
  page("guide/USE_SCENARIOS.md", "guide/use-scenarios.html", "Use Scenarios", "Guide"),
  page("spec/README.md", "spec/index.html", "Specification", "Reference"),
  ...SPEC_CHAPTERS.map(([file, title], index) =>
    page(`spec/${file}.md`, `spec/${file}.html`, `${index + 1}. ${title}`, "Reference"),
  ),
  {
    ...page("spec/GLOSSARY.md", "spec/glossary.html", "Glossary", "Reference"),
    generated: "glossary",
  },
  page("spec/STYLE.md", "spec/style.html", "Specification Style Guide", "Reference"),
  page("spec/std/README.md", "spec/std/index.html", "Standard Library", "Standard Library"),
  ...STD_CHAPTERS.map(([module, title]) =>
    page(`spec/std/${module}.md`, `spec/std/${module}.html`, title, "Standard Library"),
  ),
  page("future-work/ROADMAP.md", "roadmap.html", "Roadmap", "Project"),
];

/** The playground wrapper page, generated rather than rendered from Markdown. */
export const PLAYGROUND_PAGE = "playground.html";

/** Where a copied playground build lives in the site output. */
export const PLAYGROUND_APP_DIR = "playground";

export function navSections(): NavSection[] {
  const sections: NavSection[] = [];
  for (const entry of PAGES) {
    const last = sections.at(-1);
    if (last && last.title === entry.section) (last.pages as PageSource[]).push(entry);
    else sections.push({ title: entry.section, pages: [entry] });
  }
  return sections;
}

export const pageBySource: ReadonlyMap<string, PageSource> = new Map(
  PAGES.map((entry) => [entry.source, entry]),
);
