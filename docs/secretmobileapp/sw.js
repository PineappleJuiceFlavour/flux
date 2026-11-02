// SquadUp! service worker: makes the app load offline and start fast.
// App files are cached; calls to the SquadUp backend (another origin) always go to the network.
const VERSION = 'squadup-53456612b090';
const BASE = '/secretmobileapp'; // '' at the site root, or e.g. '/secretmobileapp'
const SHELL = ['/', '/manifest.webmanifest', '/icons/icon-192.png', '/icons/icon-512.png', '/icons/apple-touch-icon.png'].map((p) => BASE + p);
// Filled in by scripts/pwa-postbuild.js with every built file (all screens, fonts, images).
const PRECACHE = ["/secretmobileapp/_expo/static/js/web/[id]-03766fbb68034ea91e875d17a5215037.js","/secretmobileapp/_expo/static/js/web/[id]-08e4e6ec7dd55eee974d5cdbcd4de17b.js","/secretmobileapp/_expo/static/js/web/[id]-46677310a08cd0ba998680f5a4ea2a54.js","/secretmobileapp/_expo/static/js/web/[id]-6f55c036f8aecc02f551a504c3e41ca0.js","/secretmobileapp/_expo/static/js/web/__common-c5febb0c71f7fb7f61670ba0a63fc185.js","/secretmobileapp/_expo/static/js/web/__expo-metro-runtime-8c088df87a31e4e6accd81c554ec290d.js","/secretmobileapp/_expo/static/js/web/_layout-4560145903f930a034055c54a6f7ef6c.js","/secretmobileapp/_expo/static/js/web/_layout-4c24a3b3bdefa18c30cb1049fa7e9559.js","/secretmobileapp/_expo/static/js/web/add-7a19ebffc5d137329ffdd9764df3a5e5.js","/secretmobileapp/_expo/static/js/web/availability-dd4d64f618fb48e40a4cd9a5c4ecfcac.js","/secretmobileapp/_expo/static/js/web/entry-575c89aa0b73ad1f820bcd958928cb32.js","/secretmobileapp/_expo/static/js/web/index-3160143e7f7c0df5c8387b2817a7cd18.js","/secretmobileapp/_expo/static/js/web/join-3b7ca17938eae4d444ef5cb303ac83ee.js","/secretmobileapp/_expo/static/js/web/library-64e977cf070bda209d1f5703d572c5d6.js","/secretmobileapp/_expo/static/js/web/new-22cdb4449b61f33a5bb91f63eb1c8865.js","/secretmobileapp/_expo/static/js/web/new-5c774014e833cfc5e0ea351608ecba43.js","/secretmobileapp/_expo/static/js/web/news-4c706d5a66f88a1e8c46cbd6b4e001b0.js","/secretmobileapp/_expo/static/js/web/onboarding-e505fbdd90431a0bc17e657436d41ba0.js","/secretmobileapp/_expo/static/js/web/play-532fe128f491f5904339767bcd2b702f.js","/secretmobileapp/_expo/static/js/web/settings-9e16792ced6dc31418308be5b617a1c0.js","/secretmobileapp/_expo/static/js/web/squads-b22ef22e6952f4f63077fb80935c0964.js","/secretmobileapp/_expo/static/js/web/steam-callback-2bdfa9c02ada8560a5f336c2074bb40b.js","/secretmobileapp/_expo/static/js/web/steam-import-b2922144a612cf4e94935f7fc982d0c3.js","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/AntDesign.3f78af31cca60105799838a1a7a59fbd.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/Entypo.31b5ffea3daddc69dd01a1f3d6cf63c5.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/EvilIcons.140c53a7643ea949007aa9a282153849.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/Feather.ca4b48e04dc1ce10bfbddb262c8b835f.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/FontAwesome.b06871f281fee6b241d60582ae9369b9.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/FontAwesome5_Brands.3b89dd103490708d19a95adcae52210e.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/FontAwesome5_Regular.1f77739ca9ff2188b539c36f30ffa2be.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/FontAwesome5_Solid.605ed7926cf39a2ad5ec2d1f9d391d3d.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/FontAwesome6_Brands.56c8d80832e37783f12c05db7c8849e2.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/FontAwesome6_Regular.370dd5af19f8364907b6e2c41f45dbbf.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/FontAwesome6_Solid.adec7d6f310bc577f05e8fe06a5daccf.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/Fontisto.b49ae8ab2dbccb02c4d11caaacf09eab.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/Foundation.e20945d7c929279ef7a6f1db184a4470.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/Ionicons.b4eb097d35f44ed943676fd56f6bdc51.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/MaterialCommunityIcons.6e435534bd35da5fef04168860a9b8fa.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/MaterialIcons.4e85bc9ebe07e0340c9c4fc2f6c38908.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/Octicons.871378c6eab492a3e689a9385dc45a12.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/SimpleLineIcons.d2285965fe34b05465047401b8595dd0.ttf","/secretmobileapp/assets/node_modules/@expo/vector-icons/build/vendor/react-native-vector-icons/Fonts/Zocial.1681f34aaca71b8dfb70756bca331eb2.ttf","/secretmobileapp/assets/node_modules/expo-router/assets/arrow_down.017bc6ba3fc25503e5eb5e53826d48a8.png","/secretmobileapp/assets/node_modules/expo-router/assets/error.d1ea1496f9057eb392d5bbf3732a61b7.png","/secretmobileapp/assets/node_modules/expo-router/assets/file.19eeb73b9593a38f8e9f418337fc7d10.png","/secretmobileapp/assets/node_modules/expo-router/assets/forward.d8b800c443b8972542883e0b9de2bdc6.png","/secretmobileapp/assets/node_modules/expo-router/assets/pkg.ab19f4cbc543357183a20571f68380a3.png","/secretmobileapp/assets/node_modules/expo-router/assets/react-navigation/elements/back-icon-mask.0a328cd9c1afd0afe8e3b1ec5165b1b4.png","/secretmobileapp/assets/node_modules/expo-router/assets/react-navigation/elements/back-icon.35ba0eaec5a4f5ed12ca16fabeae451d.png","/secretmobileapp/assets/node_modules/expo-router/assets/react-navigation/elements/clear-icon.c94f6478e7ae0cdd9f15de1fcb9e5e55.png","/secretmobileapp/assets/node_modules/expo-router/assets/react-navigation/elements/clear-icon.c94f6478e7ae0cdd9f15de1fcb9e5e55@2x.png","/secretmobileapp/assets/node_modules/expo-router/assets/react-navigation/elements/clear-icon.c94f6478e7ae0cdd9f15de1fcb9e5e55@3x.png","/secretmobileapp/assets/node_modules/expo-router/assets/react-navigation/elements/clear-icon.c94f6478e7ae0cdd9f15de1fcb9e5e55@4x.png","/secretmobileapp/assets/node_modules/expo-router/assets/react-navigation/elements/close-icon.808e1b1b9b53114ec2838071a7e6daa7.png","/secretmobileapp/assets/node_modules/expo-router/assets/react-navigation/elements/close-icon.808e1b1b9b53114ec2838071a7e6daa7@2x.png","/secretmobileapp/assets/node_modules/expo-router/assets/react-navigation/elements/close-icon.808e1b1b9b53114ec2838071a7e6daa7@3x.png","/secretmobileapp/assets/node_modules/expo-router/assets/react-navigation/elements/close-icon.808e1b1b9b53114ec2838071a7e6daa7@4x.png","/secretmobileapp/assets/node_modules/expo-router/assets/react-navigation/elements/search-icon.286d67d3f74808a60a78d3ebf1a5fb57.png","/secretmobileapp/assets/node_modules/expo-router/assets/sitemap.412dd9275b6b48ad28f5e3d81bb1f626.png","/secretmobileapp/assets/node_modules/expo-router/assets/unmatched.20e71bdf79e3a97bf55fd9e164041578.png","/secretmobileapp/favicon.ico","/secretmobileapp/icons/apple-touch-icon.png","/secretmobileapp/icons/icon-192.png","/secretmobileapp/icons/icon-512.png","/secretmobileapp/icons/maskable-512.png","/secretmobileapp/manifest.webmanifest"];

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
