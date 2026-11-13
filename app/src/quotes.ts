/**
 * Lines from Endless Sky's own story text, shown one at a time on the start-up card.
 *
 * Source: the game's data files (GPL-3+, © Michael Zahniser and contributors), credited on the
 * card. Each was chosen to read well on its own and to give nothing away about the story:
 * generic spaceport chatter, jokes and early-game scenes only. Wording is exactly the game's
 * (a leading in-game emotion tag such as "Irony." is dropped). Before adding one, check that it
 * isn't from a late-game or plot-twist scene, and that neither the line nor its credit names a
 * species, faction or region a new player hasn't met yet (the test file keeps a list of those
 * names and fails if one slips in). A player pointed out in v0.3.1 that many of the first
 * choices did exactly that; they were removed in v0.3.2. In v0.3.3 every remaining line was
 * also checked for *where* it appears: lines that only show up in a later campaign, a
 * faction's own storyline or after a story event were swapped for ones a new player can meet
 * in human space (news chatter, hails, the intro missions and ship/outfit descriptions).
 */
export interface Quote {
  text: string;
  /** Who says it: a character's name and role, or the in-game label ("a longcow rancher"). */
  credit: string;
}

export const QUOTES: readonly Quote[] = [
  {
    text: "Risky, why? I don't see what can go wrong. The system is quite well designed. I'm so proud of my team.",
    credit: "Veronica Oxygenpocket, shipyard designer",
  },
  {
    text: "You die almost instantly, but you do have a few microseconds to reflect on the wisdom of mocking a man that is trying to fire a gun at you.",
    credit: "narration",
  },
  {
    text: "We learned years ago that we need to end every concert with a sad song. Otherwise the fans leave with way too much energy and start destroying public property.",
    credit: "Becca, band stage manager",
  },
  {
    text: "Someone with more money than sense asked me to refill the hyperspace relays, because their signal was low.",
    credit: "a hyperspace relay engineer",
  },
  {
    text: "My parents are sending me money each month, assuming that I'm studying. But I'm actually playing a space-trading game every day, all day long. Wonder how long I can keep this up.",
    credit: "a student",
  },
  {
    text: "He who can, creates; he who cannot, transports. Sorry, no offense meant to you.",
    credit: "a painter",
  },
  {
    text: "The most terrifying word in my profession is 'Oops.'",
    credit: "a nuclear technician",
  },
  {
    text: 'Someone met a djinn and said "leave me alone." That person is now stranded on an uninhabited planet.',
    credit: "a passing ship's hail",
  },
  {
    text: "Did you know that humans used to rely on explosive fuel mixtures to propel them into orbit? Imagine how mad you'd have to be to do that!",
    credit: "a museum curator",
  },
  {
    text: "Hyperspace links don't just allow faster-than-light travel. They also allow near-instantaneous communication, if you have the right sort of relay on both ends.",
    credit: "a hyperspace relay engineer",
  },
  {
    text: "I wasn't close to my parents when they died. Which was quite fortunate, since they died in an exploding ship.",
    credit: "a pirate",
  },
  {
    text: "Attention: The safe spaceship piloting training course has been canceled because our instructors crashed their shuttles this morning.",
    credit: "a spaceport announcement",
  },
  {
    text: "It kind of boggles my mind to imagine that tomorrow I'll be a dozen light-years away. We lead exciting lives, but sometimes I wonder if we don't take enough time to appreciate what's right in front of us.",
    credit: "a merchant captain",
  },
  {
    text: "Apparently the goat ate the cabbage, and shortly after that the wolf ate the goat.",
    credit: "narration",
  },
  {
    text: "Intelligent captains know staying alive is key. Intelligent captains choose to jettison their cargo. Man, I could be a Syndicate salesman!",
    credit: "a pirate's hail",
  },
  {
    text: "Syndicated Systems disapproves of space littering; your ship's wreckage will be recycled after we are finished destroying it.",
    credit: "a Syndicate warship's hail",
  },
  {
    text: "The void between the stars is cold and empty. Your ship, a tiny oasis of warmth, drifts alone out here illuminated only by the distant light of cold, uncaring suns.",
    credit: "the in-game help",
  },
  {
    text: "For reasons known only to yourself, you have spent your mortgage on three Mining Drones that are completely useless to you.",
    credit: "narration",
  },
  {
    text: "This is one of the least habitable planets in the galaxy. But if it doesn't kill us instantly, then someone's tried to settle it.",
    credit: "James, an old retired captain",
  },
  {
    text: 'Although they are typically unarmed, they have enough space for one weapon, which is the origin of the popular phrase, "as useless as a blaster cannon on a shuttlecraft."',
    credit: "the Shuttle's description",
  },
  {
    text: "Government reports indicate that as many as two out of every three first-time ship buyers who choose to pilot a Sparrow lose their ship (and often, their life as well) within the first month of owning it.",
    credit: "the Sparrow's description",
  },
  {
    text: "The enormous crate that this thruster is sold in comes decorated with a picture of a grinning Tyrannosaurus Rex with an improbable number of teeth.",
    credit: "the Tyrant Plasma Thruster's description",
  },
  {
    text: "They say that fewer than a dozen human beings alive today understand the inner workings of the hyperdrive.",
    credit: "the Hyperdrive's description",
  },
  {
    text: 'Voted the "ugliest ship in the sky" by Stars and Starships Magazine, the Protector is a typical example of brutally efficient Syndicate engineering.',
    credit: "the Protector's description",
  },
  {
    text: 'You see a box with a label "Safe for transport" that seems to cover another label saying "Warning: Explosive".',
    credit: "a spaceport cargo bay",
  },
  {
    text: "It is a soft but tightly wrapped package that makes a disquieting sound if you squeeze it.",
    credit: "narration",
  },
  {
    text: "You wind up quite a bit poorer through a combination of fines and bribes (the breakdown remains somewhat unclear).",
    credit: "narration",
  },
  {
    text: "We must make this person whole. I can't be perceived as owing favors to vagabonds. Imagine the talk!",
    credit: "a wealthy dog owner on Glory",
  },
  {
    text: "When one hatch closes, another one opens. Other than that, it's a pretty good spaceship.",
    credit: "a used spaceship salesperson",
  },
  {
    text: "The galaxy is a big and dangerous place, and not for the fainthearted. It'll be quite a while until you can make the universe your oyster.",
    credit: "an old spaceport veteran",
  },
];

/** The index shown last, so reopening the card straight away never repeats it. */
let lastIndex = -1;

/**
 * A random quote, never the same one twice in a row. `random` is injectable so tests can
 * choose; it must return a number in [0, 1) like `Math.random`.
 */
export function pickQuote(random: () => number = Math.random): Quote {
  let index = Math.min(QUOTES.length - 1, Math.floor(random() * QUOTES.length));
  if (index === lastIndex && QUOTES.length > 1) {
    index = (index + 1) % QUOTES.length;
  }
  lastIndex = index;
  return QUOTES[index];
}

/** Test hook: forgets the last quote shown. */
export function resetLastQuote(): void {
  lastIndex = -1;
}
