# Kapsule

Dépôt : [redbug26/kapsule-rust](https://github.com/redbug26/kapsule-rust).

Lecteur Rust autonome et **implémentation de référence des Kapsules** : les
applications Lua 5.4 utilisées par KKC Rust, KKC iPad et Kommando iPad.
Le binaire embarque Lua ; aucun interpréteur Lua ni dépôt KKC n’est nécessaire
pour l’utiliser. L’interface s’affiche dans un terminal.

La documentation de développement reste dans
[`kkc-store/docs/lua-applications.md`](https://github.com/redbug26/kkc-store/blob/main/docs/lua-applications.md)
et la [référence des modules](https://github.com/redbug26/kkc-store/blob/main/docs/lua-api.md).

## Compiler et lancer

Installer la toolchain Rust stable, puis :

```sh
cargo build --release --locked --target-dir target
./target/release/kapsule examples/hello
./target/release/kapsule /chemin/vers/une/kapsule/apps.toml -- argument1 argument2
```

On peut aussi installer le lecteur avec `cargo install --path . --locked`.
Sur Linux, la compilation audio nécessite les en-têtes ALSA (`libasound2-dev`
sous Debian/Ubuntu). Le binaire compilé dépend des bibliothèques système audio
usuelles de sa plateforme ; il n’inclut pas les autres projets KKC.

Le chemin désigne un dossier contenant `apps.toml`, ou ce manifeste lui-même.
Le lecteur ne copie ni ne réécrit les fichiers de la Kapsule. Les modules Lua
voisins sont importables par `require`. Les chemins relatifs de `kkc-fs` partent
du dossier de la Kapsule ; `kkc.get_cwd()` et `kkc-shell` utilisent le dossier
de lancement, modifiable avec `--cwd DOSSIER`.

Le lecteur démarre en plein écran. **F5** alterne avec la fenêtre définie dans
le manifeste ; `--windowed` démarre dans cette fenêtre. **Ctrl-C** quitte le
lecteur, y compris dans les dialogues. **Esc** est transmis à l’application.
Clavier, répétitions, relâchements si le terminal les expose, focus, souris,
couleurs RGB et barre de raccourcis sont pris en charge. L’état du terminal
est restauré à la sortie et lors d’une erreur Lua.

## Vérifier une Kapsule

```sh
kapsule --check /chemin/vers/kapsule
kapsule --headless 60 --dump-screen /chemin/vers/kapsule -- argument1
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
python3 tools/test_terminal.py
```

`--check` valide le manifeste, le chemin d’entrée et la syntaxe Lua sans
exécuter le script. `--headless N` appelle `init`, `resize`, puis jusqu’à N
cycles `update`/`draw`/`shortcuts`, dans un terminal virtuel 80×24, avec un pas
fixe `1/fps`. Ce mode n’injecte pas d’événements et ne rend pas le temps ou le
générateur aléatoire déterministes. `--dump-screen` imprime le buffer Lua final
sans bordure ni barre de raccourcis de l’hôte. Les dialogues interactifs
nécessitent un terminal et lèvent une erreur en mode headless.

Les tests incluent huit Kapsules copiées du store sans modification ; ils
vérifient leur chargement et leurs premières images, les modules, les arguments,
les fichiers, les erreurs et la CLI. Le test PTY vérifie les entrées et la
restauration du terminal. Les fixtures sont des instantanés de compatibilité,
pas des applications installées automatiquement.

## Compatibilité et suivi

Ce dépôt fait autorité pour le manifeste `apps.toml` (`[application]` et
`[runtime]`), les callbacks et les neuf modules historiques `kkc-*`. Le moteur
initial provient de `kkc-rust/src/lua_apps.rs`. Les noms Lua restent inchangés.

- `src/runtime.rs` : chargement, modules, événements, grille et boucle.
- `src/lua_dialog.rs` : dialogues bloquants, sélection filtrable et cases.
- `src/audio.rs` : lecture audio via Rodio/Symphonia.
- `tests/compatibility.rs` et `tools/test_terminal.py` : vérifications communes.

L’audio intégré lit les formats reconnus par Symphonia, notamment WAV, MP3,
FLAC et Ogg. Les décodeurs tracker et les plugins natifs de KKC ne sont pas
inclus ; un format non pris en charge produit une erreur Lua. Les deux thèmes
historiques des palettes de dialogues partagent ici le style beige du lecteur.
La souris conserve les coordonnées globales du terminal, indexées à partir de 1,
comme le contrat existant. Les fonctions de fichiers et de processus ont les
droits de l’utilisateur ; les Kapsules doivent provenir d’une source de confiance.

Toute évolution du contrat doit être documentée dans **kkc-store**, couverte
par une vérification de compatibilité et reportée dans les trois hôtes :
`../../kkc/kkc-rust`, `../../ios/kkc` et `../../ios/commando`. Ceux-ci suivent
ce moteur pour les Kapsules ; les adaptations de plateforme conservent les
noms et signatures et signalent explicitement les fonctionnalités indisponibles.

Pour des déplacements plus fluides dans les jeux, utilisez un terminal qui
prend en charge le relâchement des touches (**key up**). Terminal macOS ne
transmet pas ces événements. Le lecteur affiche un avertissement au lancement
lorsque leur prise en charge n’est pas détectée ; Entrée ou Esc permet de continuer.

Avec `key up` négocié, `is_down` reste vrai jusqu’au relâchement ou à la perte
de focus, y compris pendant le délai précédant la première répétition système.
Le lecteur ne génère pas de répétitions clavier.

Dans iTerm, le profil doit autoriser les applications à changer le mode de
clavier : **Settings > Profiles > Keys > General > Apps can change how keys
are reported**. Voir la [documentation iTerm](https://iterm2.com/documentation-one-page.html#keys-profile-preferences).
L’avertissement au lancement indique que le lecteur n’a pas pu activer le mode
avec relâchements ; changer de terminal ne suffit pas si son profil le bloque.
