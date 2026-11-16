import { coverAssetUrl } from '../../api/types';
import elden_ring from '../../assets/signin-covers/elden-ring.jpg';
import hades from '../../assets/signin-covers/hades.jpg';
import baldurs_gate_iii from '../../assets/signin-covers/baldurs-gate-iii.jpg';
import hollow_knight from '../../assets/signin-covers/hollow-knight.jpg';
import cyberpunk_2077 from '../../assets/signin-covers/cyberpunk-2077.jpg';
import stardew_valley from '../../assets/signin-covers/stardew-valley.jpg';
import portal_2 from '../../assets/signin-covers/portal-2.jpg';
import the_witcher_3_wild_hunt from '../../assets/signin-covers/the-witcher-3-wild-hunt.jpg';
import grand_theft_auto_v from '../../assets/signin-covers/grand-theft-auto-v.jpg';
import celeste from '../../assets/signin-covers/celeste.jpg';
import terraria from '../../assets/signin-covers/terraria.jpg';
import doom_eternal from '../../assets/signin-covers/doom-eternal.jpg';
import dark_souls_iii from '../../assets/signin-covers/dark-souls-iii.jpg';
import mass_effect_2 from '../../assets/signin-covers/mass-effect-2.jpg';
import disco_elysium from '../../assets/signin-covers/disco-elysium.jpg';
import outer_wilds from '../../assets/signin-covers/outer-wilds.jpg';
import ori_and_the_blind_forest from '../../assets/signin-covers/ori-and-the-blind-forest.jpg';
import undertale from '../../assets/signin-covers/undertale.jpg';
import slay_the_spire from '../../assets/signin-covers/slay-the-spire.jpg';
import sekiro_shadows_die_twice from '../../assets/signin-covers/sekiro-shadows-die-twice.jpg';
import the_last_of_us_part_ii_remastered from '../../assets/signin-covers/the-last-of-us-part-ii-remastered.jpg';
import factorio from '../../assets/signin-covers/factorio.jpg';
import vampire_survivors from '../../assets/signin-covers/vampire-survivors.jpg';
import death_stranding_directors_cut from '../../assets/signin-covers/death-stranding-directors-cut.jpg';
import control from '../../assets/signin-covers/control.jpg';

/**
 * Real IGDB cover art bundled with the app for the signed-out mural fallback.
 *
 * Each entry keeps the catalogue title and the IGDB CDN image URL in
 * `assets/signin-covers/manifest.json`, so the artwork can be audited without
 * inferring a game from an image hash. The user's cover cache always comes
 * first; these files only fill the remaining tiles.
 */
export interface CuratedSignInCover {
  slug: string;
  title: string;
  localFile: string;
  gameId: number;
  releaseYear: number;
  igdbImageUrl: string;
  /** Vite URL of the bundled image (hashed in a production build). */
  src: string;
}

export const CURATED_SIGN_IN_COVERS: readonly CuratedSignInCover[] = [
  {
    slug: "elden-ring",
    title: "Elden Ring",
    localFile: "elden-ring.jpg",
    gameId: 119133,
    releaseYear: 2022,
    igdbImageUrl: "https://images.igdb.com/igdb/image/upload/t_cover_big/co4jni.jpg",
    src: elden_ring,
  },
  {
    slug: "hades",
    title: "Hades",
    localFile: "hades.jpg",
    gameId: 113112,
    releaseYear: 2020,
    igdbImageUrl: "https://images.igdb.com/igdb/image/upload/t_cover_big/cob9kr.jpg",
    src: hades,
  },
  {
    slug: "baldurs-gate-iii",
    title: "Baldur's Gate III",
    localFile: "baldurs-gate-iii.jpg",
    gameId: 119171,
    releaseYear: 2023,
    igdbImageUrl: "https://images.igdb.com/igdb/image/upload/t_cover_big/co670h.jpg",
    src: baldurs_gate_iii,
  },
  {
    slug: "hollow-knight",
    title: "Hollow Knight",
    localFile: "hollow-knight.jpg",
    gameId: 14593,
    releaseYear: 2017,
    igdbImageUrl: "https://images.igdb.com/igdb/image/upload/t_cover_big/cobfzp.jpg",
    src: hollow_knight,
  },
  {
    slug: "cyberpunk-2077",
    title: "Cyberpunk 2077",
    localFile: "cyberpunk-2077.jpg",
    gameId: 1877,
    releaseYear: 2020,
    igdbImageUrl: "https://images.igdb.com/igdb/image/upload/t_cover_big/coaih8.jpg",
    src: cyberpunk_2077,
  },
  {
    slug: "stardew-valley",
    title: "Stardew Valley",
    localFile: "stardew-valley.jpg",
    gameId: 17000,
    releaseYear: 2016,
    igdbImageUrl: "https://images.igdb.com/igdb/image/upload/t_cover_big/coa93h.jpg",
    src: stardew_valley,
  },
  {
    slug: "portal-2",
    title: "Portal 2",
    localFile: "portal-2.jpg",
    gameId: 72,
    releaseYear: 2011,
    igdbImageUrl: "https://images.igdb.com/igdb/image/upload/t_cover_big/co1rs4.jpg",
    src: portal_2,
  },
  {
    slug: "the-witcher-3-wild-hunt",
    title: "The Witcher 3: Wild Hunt",
    localFile: "the-witcher-3-wild-hunt.jpg",
    gameId: 1942,
    releaseYear: 2015,
    igdbImageUrl: "https://images.igdb.com/igdb/image/upload/t_cover_big/coaarl.jpg",
    src: the_witcher_3_wild_hunt,
  },
  {
    slug: "grand-theft-auto-v",
    title: "Grand Theft Auto V",
    localFile: "grand-theft-auto-v.jpg",
    gameId: 1020,
    releaseYear: 2013,
    igdbImageUrl: "https://images.igdb.com/igdb/image/upload/t_cover_big/co2lbd.jpg",
    src: grand_theft_auto_v,
  },
  {
    slug: "celeste",
    title: "Celeste",
    localFile: "celeste.jpg",
    gameId: 26226,
    releaseYear: 2018,
    igdbImageUrl: "https://images.igdb.com/igdb/image/upload/t_cover_big/cob9dh.jpg",
    src: celeste,
  },
  {
    slug: "terraria",
    title: "Terraria",
    localFile: "terraria.jpg",
    gameId: 1879,
    releaseYear: 2011,
    igdbImageUrl: "https://images.igdb.com/igdb/image/upload/t_cover_big/coaamg.jpg",
    src: terraria,
  },
  {
    slug: "doom-eternal",
    title: "Doom Eternal",
    localFile: "doom-eternal.jpg",
    gameId: 103298,
    releaseYear: 2020,
    igdbImageUrl: "https://images.igdb.com/igdb/image/upload/t_cover_big/co3p5n.jpg",
    src: doom_eternal,
  },
  {
    slug: "dark-souls-iii",
    title: "Dark Souls III",
    localFile: "dark-souls-iii.jpg",
    gameId: 11133,
    releaseYear: 2016,
    igdbImageUrl: "https://images.igdb.com/igdb/image/upload/t_cover_big/cob9ed.jpg",
    src: dark_souls_iii,
  },
  {
    slug: "mass-effect-2",
    title: "Mass Effect 2",
    localFile: "mass-effect-2.jpg",
    gameId: 74,
    releaseYear: 2010,
    igdbImageUrl: "https://images.igdb.com/igdb/image/upload/t_cover_big/co20ac.jpg",
    src: mass_effect_2,
  },
  {
    slug: "disco-elysium",
    title: "Disco Elysium",
    localFile: "disco-elysium.jpg",
    gameId: 26472,
    releaseYear: 2019,
    igdbImageUrl: "https://images.igdb.com/igdb/image/upload/t_cover_big/co1sfj.jpg",
    src: disco_elysium,
  },
  {
    slug: "outer-wilds",
    title: "Outer Wilds",
    localFile: "outer-wilds.jpg",
    gameId: 11737,
    releaseYear: 2019,
    igdbImageUrl: "https://images.igdb.com/igdb/image/upload/t_cover_big/co65ac.jpg",
    src: outer_wilds,
  },
  {
    slug: "ori-and-the-blind-forest",
    title: "Ori and the Blind Forest",
    localFile: "ori-and-the-blind-forest.jpg",
    gameId: 7344,
    releaseYear: 2015,
    igdbImageUrl: "https://images.igdb.com/igdb/image/upload/t_cover_big/co1y41.jpg",
    src: ori_and_the_blind_forest,
  },
  {
    slug: "undertale",
    title: "Undertale",
    localFile: "undertale.jpg",
    gameId: 12517,
    releaseYear: 2015,
    igdbImageUrl: "https://images.igdb.com/igdb/image/upload/t_cover_big/cob1t2.jpg",
    src: undertale,
  },
  {
    slug: "slay-the-spire",
    title: "Slay the Spire",
    localFile: "slay-the-spire.jpg",
    gameId: 40477,
    releaseYear: 2019,
    igdbImageUrl: "https://images.igdb.com/igdb/image/upload/t_cover_big/co1iyf.jpg",
    src: slay_the_spire,
  },
  {
    slug: "sekiro-shadows-die-twice",
    title: "Sekiro: Shadows Die Twice",
    localFile: "sekiro-shadows-die-twice.jpg",
    gameId: 76882,
    releaseYear: 2019,
    igdbImageUrl: "https://images.igdb.com/igdb/image/upload/t_cover_big/co2a23.jpg",
    src: sekiro_shadows_die_twice,
  },
  {
    slug: "the-last-of-us-part-ii-remastered",
    title: "The Last of Us Part II Remastered",
    localFile: "the-last-of-us-part-ii-remastered.jpg",
    gameId: 277143,
    releaseYear: 2024,
    igdbImageUrl: "https://images.igdb.com/igdb/image/upload/t_cover_big/coa1gr.jpg",
    src: the_last_of_us_part_ii_remastered,
  },
  {
    slug: "factorio",
    title: "Factorio",
    localFile: "factorio.jpg",
    gameId: 7046,
    releaseYear: 2020,
    igdbImageUrl: "https://images.igdb.com/igdb/image/upload/t_cover_big/co1tfy.jpg",
    src: factorio,
  },
  {
    slug: "vampire-survivors",
    title: "Vampire Survivors",
    localFile: "vampire-survivors.jpg",
    gameId: 186725,
    releaseYear: 2022,
    igdbImageUrl: "https://images.igdb.com/igdb/image/upload/t_cover_big/co4bzv.jpg",
    src: vampire_survivors,
  },
  {
    slug: "death-stranding-directors-cut",
    title: "Death Stranding: Director's Cut",
    localFile: "death-stranding-directors-cut.jpg",
    gameId: 152063,
    releaseYear: 2021,
    igdbImageUrl: "https://images.igdb.com/igdb/image/upload/t_cover_big/cobksg.jpg",
    src: death_stranding_directors_cut,
  },
  {
    slug: "control",
    title: "Control",
    localFile: "control.jpg",
    gameId: 103329,
    releaseYear: 2019,
    igdbImageUrl: "https://images.igdb.com/igdb/image/upload/t_cover_big/co2evj.jpg",
    src: control,
  },
];

/** How many tiles the decorative mural renders (5 columns × 5 rows). */
export const SIGN_IN_MURAL_TILES = 25;

const MOCK_MURAL_PLACEHOLDER = /^mock-\d+\.svg$/;

export interface ResolvedMuralCover {
  src: string;
  source: 'cache' | 'curated';
  /** Set when the tile is one of the bundled samples. */
  curated: CuratedSignInCover | null;
}

function validCoverFile(file: string): boolean {
  if (!file || file.startsWith('.')) return false;
  return (
    !file.includes('/') &&
    !file.includes('\\') &&
    !file.includes(':') &&
    !file.includes('\0') &&
    !file.includes('..')
  );
}

/**
 * Resolve the mural from the host's cover-wall names.
 *
 * Real cache files keep their custom-protocol URL and their position. The mock
 * host's `mock-N.svg` placeholders, invalid names, and every unused slot are
 * filled with bundled real IGDB artwork, so a first install never renders an
 * empty neutral wall and the mock never shows the placeholder SVGs.
 */
export function resolveMuralCovers(files: readonly string[]): ResolvedMuralCover[] {
  const resolved: Array<ResolvedMuralCover | null> = Array.from(
    { length: SIGN_IN_MURAL_TILES },
    () => null,
  );
  let curatedIndex = 0;
  const nextCurated = (): ResolvedMuralCover => {
    const curated = CURATED_SIGN_IN_COVERS[curatedIndex % CURATED_SIGN_IN_COVERS.length]!;
    curatedIndex += 1;
    return { src: curated.src, source: 'curated', curated };
  };

  files.slice(0, SIGN_IN_MURAL_TILES).forEach((file, index) => {
    if (MOCK_MURAL_PLACEHOLDER.test(file) || !validCoverFile(file)) {
      resolved[index] = nextCurated();
      return;
    }
    const src = coverAssetUrl(file);
    resolved[index] = src
      ? { src, source: 'cache', curated: null }
      : nextCurated();
  });

  return resolved.map((entry) => entry ?? nextCurated());
}
