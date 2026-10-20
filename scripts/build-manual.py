#!/usr/bin/env python3
"""Write the website manual from the editor's help topics (crates/rysmo_app/src/help.rs).

Usage: build-manual.py OUT_DIR. Writes OUT_DIR/index.html, which links ../site.css and ../theme.js.
"""
import html
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
HELP = ROOT / "crates/rysmo_app/src/help.rs"
INPUT = ROOT / "crates/rysmo_app/src/input.rs"

# A Rust string literal: raw r###"..."### or plain "...".
STRING = r'(r###"(?:.|\n)*?"###|"(?:[^"\\]|\\.)*")'
TOPIC = re.compile(
    r"Topic \{\s*title: " + STRING + r",\s*category: " + STRING
    + r",\s*icon: (None|Some\(Icon::\w+\((?:ToolKind|Action)::(\w+)\)\)),\s*body: " + STRING + r",\s*\}"
)


def rust_string(literal):
    if literal.startswith('r###"'):
        return literal[5:-4]
    return re.sub(r"\\(.)", lambda m: {"n": "\n", "t": "\t"}.get(m.group(1), m.group(1)), literal[1:-1])


def topics():
    source = HELP.read_text()
    table = source[source.index("static TOPICS"):source.index("\n];", source.index("static TOPICS"))]
    found = [
        {"title": rust_string(m[1]), "category": rust_string(m[2]), "tool": m[4], "body": rust_string(m[5])}
        for m in TOPIC.finditer(table)
    ]
    expected = table.count("Topic {")
    if len(found) != expected:
        sys.exit(f"build-manual: parsed {len(found)} of {expected} help topics; help.rs changed shape")
    return found, source


def categories(source, found):
    """The order the in-app help lists categories in, then any category it does not list."""
    listed = re.search(r"for category in \[(.*?)\]", source, re.S)
    order = re.findall(r'"([^"]+)"', listed[1]) if listed else []
    for topic in found:
        if topic["category"] not in order:
            order.append(topic["category"])
    return order


def shortcuts():
    source = INPUT.read_text()
    block = source[source.index("fn shortcut(c: char)"):]
    block = block[:block.index("\n}")]
    keys = {}
    for key, tool in re.findall(r"'(.)' => ToolKind::(\w+)", block):
        keys[tool] = "Space" if key == " " else key.upper()
    return keys


def slug(text):
    return re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")


PAGE = """<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Rysmo manual</title>
  <meta name="description" content="How to use Rysmo: tools, shortcuts, measurements, materials, terrain and files.">
  <link rel="icon" type="image/svg+xml" href="../favicon.svg">
  <link rel="stylesheet" href="../site.css">
  <script>
    // Apply the saved theme before the first paint; without one the page follows the system.
    document.documentElement.classList.add('js');
    try {{ var t = localStorage.getItem('rysmo-theme'); if (t) document.documentElement.dataset.theme = t; }} catch (e) {{}}
  </script>
  <script src="../theme.js" defer></script>
</head>
<body>
  <div class="top">
    <div class="wrap">
      <header>
        <a class="brand" href="../"><img src="../logo.svg" alt="">Rysmo</a>
        <nav>
          <a href="./" aria-current="page">Manual</a>
          <a class="github" href="https://github.com/MojzisekJan/rysmo">GitHub</a>
          <button class="theme-toggle" type="button" aria-label="Switch theme">
            <svg class="sun" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><circle cx="12" cy="12" r="4"/><path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M5.3 18.7l1.4-1.4M17.3 6.7l1.4-1.4"/></svg>
            <svg class="moon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"><path d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z"/></svg>
          </button>
          <a class="button primary" href="../editor/">Open editor</a>
        </nav>
      </header>
    </div>
  </div>

  <div class="wrap manual">
    <aside class="manual-nav">
      <input class="manual-search" type="search" placeholder="Search {count} topics" aria-label="Search the manual" hidden>
{toc}
    </aside>

    <main class="manual-main">
      <h1>Manual</h1>
      <p class="no-results" hidden>No matching topics. Try a tool name, a shortcut or a shorter search.</p>
{sections}
    </main>
  </div>

  <div class="wrap">
    <footer>
      <span>Rysmo is free software under the GPL-3.0-or-later.</span>
      <a href="https://github.com/MojzisekJan/rysmo">github.com/MojzisekJan/rysmo</a>
    </footer>
  </div>

  <script>
    // Search keeps the topics that contain every typed word, as the search in the app does.
    (function () {{
      var input = document.querySelector('.manual-search');
      var topics = [].slice.call(document.querySelectorAll('.topic'));
      var empty = document.querySelector('.no-results');
      input.hidden = false;
      input.addEventListener('input', function () {{
        var words = input.value.toLowerCase().split(/\\s+/).filter(Boolean), shown = 0;
        topics.forEach(function (topic) {{
          var text = topic.dataset.category + ' ' + topic.textContent.toLowerCase();
          var match = words.every(function (w) {{ return text.indexOf(w) >= 0; }});
          topic.hidden = !match;
          var link = document.querySelector('.toc a[href="#' + topic.id + '"]');
          if (link) link.parentNode.hidden = !match;
          if (match) shown++;
        }});
        [].forEach.call(document.querySelectorAll('.manual-category, .toc'), function (group) {{
          group.hidden = !group.querySelector('.topic:not([hidden]), li:not([hidden])');
        }});
        empty.hidden = shown > 0;
      }});
    }})();
  </script>
</body>
</html>
"""


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__.strip())
    found, source = topics()
    keys = shortcuts()
    ids = set()
    toc, sections = [], []
    for category in categories(source, found):
        group = [t for t in found if t["category"] == category]
        if not group:
            continue
        links, articles = [], []
        for topic in group:
            anchor = slug(topic["title"])
            while anchor in ids:
                anchor += "-"
            ids.add(anchor)
            title = html.escape(topic["title"])
            key = keys.get(topic["tool"])
            kbd = f" <kbd>{key}</kbd>" if key else ""
            body = "\n".join(f"        <p>{html.escape(p.strip())}</p>" for p in topic["body"].split("\n") if p.strip())
            links.append(f'          <li><a href="#{anchor}">{title}</a></li>')
            articles.append(
                f'      <article class="topic" id="{anchor}" data-category="{html.escape(category.lower())}">\n'
                f'        <h3>{title}{kbd} <a class="anchor" href="#{anchor}" aria-label="Link to {title}">#</a></h3>\n'
                f"{body}\n      </article>"
            )
        name = html.escape(category)
        toc.append(f'      <div class="toc">\n        <h2>{name}</h2>\n        <ul>\n' + "\n".join(links) + "\n        </ul>\n      </div>")
        sections.append(f'      <section class="manual-category">\n        <h2>{name}</h2>\n' + "\n".join(articles) + "\n      </section>")
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    (out / "index.html").write_text(PAGE.format(count=len(found), toc="\n".join(toc), sections="\n".join(sections)))
    print(f"Wrote {out / 'index.html'} with {len(found)} topics.")


if __name__ == "__main__":
    main()
