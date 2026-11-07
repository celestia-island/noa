# Fusion et conflits

La fusion de noa est une structure à deux étages « récursion par arbre + feuilles par ligne » : les répertoires sont parcourus récursivement niveau par niveau, et un conflit au niveau fichier tente d'abord une fusion tri-directionnelle au niveau des lignes (diff3), avant de retomber sur un conflit structuré.

## Fusion tri-directionnelle au niveau des lignes (W2④)

Pour des modifications des deux côtés d'un même fichier, on commence par un balayage diff3 avec l'ancêtre commun comme base :

- **Zones touchées par un seul côté** : on prend ce côté tel quel — les éditions non chevauchantes fusionnent automatiquement, sans plus produire de conflit sur tout le blob ;
- **Zones chevauchantes ou adjacentes** (comme git, même sans espace entre elles, cela compte comme conflit) : génération d'un bloc de marqueurs de conflit à la git
  (`<<<<<<< ours` / `=======` / `>>>>>>> theirs`), en conservant en parallèle le conflit structuré pour résolution dans le panneau ;
- **Contenu identique ou un côté inchangé** : chemin rapide qui prend directement le côté disponible.

```rust
use libnoa::merge::line_merge::{merge_lines, LineMerge};

match merge_lines(base, ours, theirs) {
    LineMerge::Clean(merged) => { /* auto-resolved */ }
    LineMerge::Conflict { merged, ours, theirs, base } => { /* structured conflict */ }
}
```

## Renommage par contenu exact (W2④, deuxième passe)

Quand un côté fait un « renommage pur » (l'ancien chemin est supprimé et le contenu du blob réapparaît tel quel au nouveau chemin) et que l'autre côté **modifie l'ancien chemin**, la modification est automatiquement appliquée au **nouveau chemin** — le scénario classique rename+edit ne produit plus de conflit « suppression vs modification ».

Cas non déclarés (repli conservateur sur conflit structuré) :

- renommage accompagné d'un changement de contenu (rename chargé) ;
- les deux côtés renomment vers des cibles différentes ;
- le chemin cible est touché par l'autre côté (collision) ;
- rename vs delete (le rename transporte le contenu de base et survit au nouveau chemin — sémantique rename-aware de git).

L'heuristique de similarité (reconnaissance des renommages sans correspondance exacte) est une tranche ultérieure.

## Sémantique de repli

Quand la fusion au niveau des lignes est indisponible (blob manquant/endommagé/binaire/contenant un CR isolé), le fichier retombe sur un conflit structuré de tout le blob
— jamais de suppression silencieuse du fichier, jamais de 500. La forme de fixture aux trois côtés vides (le stockage matérialise les id manquants en blobs vides) se replie de la même façon.

## Flux de fusion des PR

`POST /api/v1/repos/{name}/prs/{number}/merge` :

1. fusion tri-directionnelle des instantanés (base = base de la PR, ours = espace de travail cible, theirs = head de la PR) ;
2. sans conflit → écriture automatique de l'arbre + avance des références ;
3. avec conflit → 409 + liste des conflits (chemin + ids de blob ours/theirs/base), que le panneau résout via
   `ConflictResolution::{Ours, Theirs}`.

## Compatibilité git

Les entrées blob du résultat de fusion satisfont l'algèbre d'objets de git (OID de blob
identiques octet par octet). Mais **l'arbre résolu lui-même est natif de noa** (adressage
SHA-256 propre à noa) : il n'apparaît pas automatiquement dans le scratch — le renvoyer
vers l'origine exige une sérialisation instantané → objets git (l'export au niveau objet
est une tranche ultérieure ; la voie d'export de l'arbre de travail existante permet de
faire la même chose manuellement). D'ici là, les résultats de fusion vivent dans le
système de références de noa, lisibles comme d'habitude via espaces de travail/instantanés.
