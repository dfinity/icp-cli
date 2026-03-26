var _paq = (window._paq = window._paq || []);
_paq.push(["disableCookies"]);
_paq.push(["enableLinkTracking"]);
_paq.push(["trackPageView"]);
(function () {
  var u = "https://internetcomputer.matomo.cloud/";
  _paq.push(["setTrackerUrl", u + "matomo.php"]);
  _paq.push(["setSiteId", "CHANGEME"]);
  var d = document,
    g = d.createElement("script"),
    s = d.getElementsByTagName("script")[0];
  g.async = true;
  g.src = "//cdn.matomo.cloud/internetcomputer.matomo.cloud/matomo.js";
  s.parentNode.insertBefore(g, s);
})();
