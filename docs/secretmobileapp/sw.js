// SquadUp! service worker: makes the app load offline and start fast.
// App files are cached; calls to the SquadUp backend (another origin) always go to the network.
const VERSION = 'squadup-b029f4843daf';
const BASE = '/secretmobileapp'; // '' at the site root, or e.g. '/secretmobileapp'
const SHELL = ['/', '/manifest.webmanifest', '/icons/icon-192.png', '/icons/icon-512.png', '/icons/apple-touch-icon.png'].map((p) => BASE + p);
// Filled in by scripts/pwa-postbuild.js with every built file (all screens, fonts, images).
const PRECACHE = ["/secretmobileapp/_expo/static/js/web/[id]-128d4e252926eee6f96b5b6585913651.js","/secretmobileapp/_expo/static/js/web/[id]-41dc1c79cd714542d2827c065cb43639.js","/secretmobileapp/_expo/static/js/web/[id]-9193216ae99293faba6a1b95a7b12d1e.js","/secretmobileapp/_expo/static/js/web/[id]-f63d8471a3ee578b0c89a7ef2a1e44f2.js","/secretmobileapp/_expo/static/js/web/__common-1cc6612a2a6425fdd757c9f8fa559358.js","/secretmobileapp/_expo/static/js/web/__expo-metro-runtime-8c088df87a31e4e6accd81c554ec290d.js","/secretmobileapp/_expo/static/js/web/_layout-4323d7efcf6d501f3c997ed7d274f22e.js","/secretmobileapp/_expo/static/js/web/_layout-4560145903f930a034055c54a6f7ef6c.js","/secretmobileapp/_expo/static/js/web/add-434e7f870bc607247e58acd9a1c8df80.js","/secretmobileapp/_expo/static/js/web/availability-e5caac4461463c52dd7bb1f485253c82.js","/secretmobileapp/_expo/static/js/web/entry-bda47e5095a82e93917d0bf6c92b1738.js","/secretmobileapp/_expo/static/js/web/index-bcef0e9d8e1c1a5a4b295efcc864aaf7.js","/secretmobileapp/_expo/static/js/web/join-8271280e07b32f8a6c851a279f130aa5.js","/secretmobileapp/_expo/static/js/web/library-6057e6c5bc75d3759314caddb99908ab.js","/secretmobileapp/_expo/static/js/web/new-3fb32f6a612940fc558c4e667a2c2a46.js","/secretmobileapp/_expo/static/js/web/new-c67dc8f6d6588fe50df3d5198dd9bed4.js","/secretmobileapp/_expo/static/js/web/news-ff38db2ca7dd16fb379fcdc8f79b0a5f.js","/secretmobileapp/_expo/static/js/web/onboarding-fcbb54b0f550a1a4e28ceeff9564ccb3.js","/secretmobileapp/_expo/static/js/web/play-70541a244ca08ad2898de09b26b55af9.js","/secretmobileapp/_expo/static/js/web/settings-9d9bb933598378c04e220208a0b3f0cf.js","/secretmobileapp/_expo/static/js/web/squads-6cff3023d773872dba8e74153e285d1a.js","/secretmobileapp/_expo/static/js/web/steam-callback-2bdfa9c02ada8560a5f336c2074bb40b.js","/secretmobileapp/_expo/static/js/web/steam-import-52e1fc03be929fbcd8cdbd910a7be9e1.js","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/AntDesign.3f78af31cca60105799838a1a7a59fbd.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/Entypo.31b5ffea3daddc69dd01a1f3d6cf63c5.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/EvilIcons.140c53a7643ea949007aa9a282153849.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/Feather.ca4b48e04dc1ce10bfbddb262c8b835f.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/FontAwesome.b06871f281fee6b241d60582ae9369b9.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/FontAwesome5_Brands.3b89dd103490708d19a95adcae52210e.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/FontAwesome5_Regular.1f77739ca9ff2188b539c36f30ffa2be.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/FontAwesome5_Solid.605ed7926cf39a2ad5ec2d1f9d391d3d.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/FontAwesome6_Brands.56c8d80832e37783f12c05db7c8849e2.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/FontAwesome6_Regular.370dd5af19f8364907b6e2c41f45dbbf.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/FontAwesome6_Solid.adec7d6f310bc577f05e8fe06a5daccf.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/Fontisto.b49ae8ab2dbccb02c4d11caaacf09eab.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/Foundation.e20945d7c929279ef7a6f1db184a4470.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/Ionicons.b4eb097d35f44ed943676fd56f6bdc51.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/MaterialCommunityIcons.6e435534bd35da5fef04168860a9b8fa.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/MaterialIcons.4e85bc9ebe07e0340c9c4fc2f6c38908.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/Octicons.871378c6eab492a3e689a9385dc45a12.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/SimpleLineIcons.d2285965fe34b05465047401b8595dd0.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/Zocial.1681f34aaca71b8dfb70756bca331eb2.ttf","/secretmobileapp/assets/node_modules/expo-router/assets/arrow_down.017bc6ba3fc25503e5eb5e53826d48a8.png","/secretmobileapp/assets/node_modules/expo-router/assets/error.d1ea1496f9057eb392d5bbf3732a61b7.png","/secretmobileapp/assets/node_modules/expo-router/assets/file.19eeb73b9593a38f8e9f418337fc7d10.png","/secretmobileapp/assets/node_modules/expo-router/assets/forward.d8b800c443b8972542883e0b9de2bdc6.png","/secretmobileapp/assets/node_modules/expo-router/assets/pkg.ab19f4cbc543357183a20571f68380a3.png","/secretmobileapp/assets/node_modules/expo-router/assets/react-navigation/elements/back-icon-mask.0a328cd9c1afd0afe8e3b1ec5165b1b4.png","/secretmobileapp/assets/node_modules/expo-router/assets/react-navigation/elements/back-icon.35ba0eaec5a4f5ed12ca16fabeae451d.png","/secretmobileapp/assets/node_modules/expo-router/assets/react-navigation/elements/clear-icon.c94f6478e7ae0cdd9f15de1fcb9e5e55.png","/secretmobileapp/assets/node_modules/expo-router/assets/react-navigation/elements/clear-icon.c94f6478e7ae0cdd9f15de1fcb9e5e55@2x.png","/secretmobileapp/assets/node_modules/expo-router/assets/react-navigation/elements/clear-icon.c94f6478e7ae0cdd9f15de1fcb9e5e55@3x.png","/secretmobileapp/assets/node_modules/expo-router/assets/react-navigation/elements/clear-icon.c94f6478e7ae0cdd9f15de1fcb9e5e55@4x.png","/secretmobileapp/assets/node_modules/expo-router/assets/react-navigation/elements/close-icon.808e1b1b9b53114ec2838071a7e6daa7.png","/secretmobileapp/assets/node_modules/expo-router/assets/react-navigation/elements/close-icon.808e1b1b9b53114ec2838071a7e6daa7@2x.png","/secretmobileapp/assets/node_modules/expo-router/assets/react-navigation/elements/close-icon.808e1b1b9b53114ec2838071a7e6daa7@3x.png","/secretmobileapp/assets/node_modules/expo-router/assets/react-navigation/elements/close-icon.808e1b1b9b53114ec2838071a7e6daa7@4x.png","/secretmobileapp/assets/node_modules/expo-router/assets/react-navigation/elements/search-icon.286d67d3f74808a60a78d3ebf1a5fb57.png","/secretmobileapp/assets/node_modules/expo-router/assets/sitemap.412dd9275b6b48ad28f5e3d81bb1f626.png","/secretmobileapp/assets/node_modules/expo-router/assets/unmatched.20e71bdf79e3a97bf55fd9e164041578.png","/secretmobileapp/favicon.ico","/secretmobileapp/icons/apple-touch-icon.png","/secretmobileapp/icons/icon-192.png","/secretmobileapp/icons/icon-512.png","/secretmobileapp/icons/maskable-512.png","/secretmobileapp/manifest.webmanifest"];

self.addEventListener('install', (event) => {
  event.waitUntil(caches.open(VERSION).then((c) => c.addAll([...new Set([...SHELL, ...PRECACHE])])).then(() => self.skipWaiting()));
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches.keys().then((keys) => Promise.all(keys.filter((k) => k !== VERSION).map((k) => caches.delete(k)))).then(() => self.clients.claim()),
  );
});

self.addEventListener('fetch', (event) => {
  const req = event.request;
  const url = new URL(req.url);
  // Only handle this app's own files; leave the rest of the website alone.
  if (req.method !== 'GET' || url.origin !== self.location.origin || !(url.pathname === BASE || url.pathname.startsWith(BASE + '/'))) return;

  // Page navigations: try the network, fall back to the cached app shell when offline.
  if (req.mode === 'navigate') {
    event.respondWith(
      fetch(req)
        .then((res) => {
          const copy = res.clone();
          caches.open(VERSION).then((c) => c.put(BASE + '/', copy));
          return res;
        })
        .catch(() => caches.match(BASE + '/')),
    );
    return;
  }

  // Bundled JS, fonts and images have content hashes in their names, so cache-first is safe.
  event.respondWith(
    caches.match(req).then(
      (hit) =>
        hit ||
        fetch(req).then((res) => {
          if (res.ok) {
            const copy = res.clone();
            caches.open(VERSION).then((c) => c.put(req, copy));
          }
          return res;
        }),
    ),
  );
});
