// The theme toggle. A choice is remembered; without one the page follows the system.
(function () {
  var root = document.documentElement, button = document.querySelector('.theme-toggle');
  var dark = matchMedia('(prefers-color-scheme: dark)');
  function stored() {
    try { return localStorage.getItem('rysmo-theme'); } catch (e) { return null; }
  }
  function show(theme) {
    root.dataset.theme = theme;
    button.setAttribute('aria-label', theme === 'dark' ? 'Switch to light theme' : 'Switch to dark theme');
    button.title = button.getAttribute('aria-label');
  }
  show(stored() || (dark.matches ? 'dark' : 'light'));
  button.addEventListener('click', function () {
    var next = root.dataset.theme === 'dark' ? 'light' : 'dark';
    show(next);
    try { localStorage.setItem('rysmo-theme', next); } catch (e) {}
  });
  dark.addEventListener('change', function (e) {
    if (!stored()) show(e.matches ? 'dark' : 'light');
  });
})();
