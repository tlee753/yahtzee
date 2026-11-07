#![windows_subsystem = "windows"]

use iced::theme::Palette;
use iced::widget::{Column, Row, button, column, container, row, text};
use iced::{
    Background, Border, Color, Element, Font, Length, Size, Subscription, Theme, alignment, time,
};
use rand::Rng;
use std::time::Duration;

/// Accent colour: #00ffaf
const ACCENT: Color = Color {
    r: 0.0,
    g: 1.0,
    b: 175.0 / 255.0,
    a: 1.0,
};

fn main() -> iced::Result {
    iced::application(Yahtzee::default, Yahtzee::update, Yahtzee::view)
        .title("Yahtzee")
        // iced can't load WOFF2 — convert it to TTF first (see README note)
        .font(include_bytes!("lexend.ttf").as_slice())
        .default_font(Font::with_name("Lexend"))
        .subscription(Yahtzee::subscription)
        .theme(Yahtzee::theme)
        .window_size(Size::new(1000.0, 820.0))
        .run()
}

// ───────────────────────────── Categories & scoring ─────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Category {
    Ones,
    Twos,
    Threes,
    Fours,
    Fives,
    Sixes,
    ThreeKind,
    FourKind,
    FullHouse,
    SmallStraight,
    LargeStraight,
    Yahtzee,
    Chance,
}

const CATEGORIES: [Category; 13] = [
    Category::Ones,
    Category::Twos,
    Category::Threes,
    Category::Fours,
    Category::Fives,
    Category::Sixes,
    Category::ThreeKind,
    Category::FourKind,
    Category::FullHouse,
    Category::SmallStraight,
    Category::LargeStraight,
    Category::Yahtzee,
    Category::Chance,
];

const YAHTZEE_IDX: usize = 11;

impl Category {
    fn index(self) -> usize {
        self as usize
    }

    fn name(self) -> &'static str {
        match self {
            Category::Ones => "Ones",
            Category::Twos => "Twos",
            Category::Threes => "Threes",
            Category::Fours => "Fours",
            Category::Fives => "Fives",
            Category::Sixes => "Sixes",
            Category::ThreeKind => "Three of a Kind",
            Category::FourKind => "Four of a Kind",
            Category::FullHouse => "Full House",
            Category::SmallStraight => "Small Straight",
            Category::LargeStraight => "Large Straight",
            Category::Yahtzee => "Yahtzee",
            Category::Chance => "Chance",
        }
    }

    /// Average score this category tends to yield under good play.
    /// The AI uses `score - baseline` to judge how "good" a placement is.
    fn baseline(self) -> f32 {
        match self {
            Category::Ones => 2.11,
            Category::Twos => 5.28,
            Category::Threes => 8.57,
            Category::Fours => 12.16,
            Category::Fives => 15.69,
            Category::Sixes => 19.19,
            Category::ThreeKind => 21.66,
            Category::FourKind => 13.10,
            Category::FullHouse => 22.59,
            Category::SmallStraight => 29.46,
            Category::LargeStraight => 32.71,
            Category::Yahtzee => 16.87,
            Category::Chance => 22.01,
        }
    }
}

fn is_yahtzee(dice: &[u8; 5]) -> bool {
    dice[0] != 0 && dice.iter().all(|&d| d == dice[0])
}

/// Points for putting `dice` in `cat`. `joker` means the dice are a Yahtzee and the
/// Yahtzee box is already filled, in which case Full House / Straights score in full.
fn points(cat: Category, dice: &[u8; 5], joker: bool) -> u32 {
    let mut counts = [0u32; 7];
    for &d in dice {
        counts[d as usize] += 1;
    }
    let sum: u32 = dice.iter().map(|&d| d as u32).sum();
    let max_count = *counts[1..].iter().max().unwrap();
    let has = |n: usize| counts[n] > 0;

    match cat {
        Category::Ones
        | Category::Twos
        | Category::Threes
        | Category::Fours
        | Category::Fives
        | Category::Sixes => {
            let face = cat.index() + 1;
            counts[face] * face as u32
        }
        Category::ThreeKind => {
            if max_count >= 3 {
                sum
            } else {
                0
            }
        }
        Category::FourKind => {
            if max_count >= 4 {
                sum
            } else {
                0
            }
        }
        Category::FullHouse => {
            if joker || (counts.contains(&3) && counts.contains(&2)) {
                25
            } else {
                0
            }
        }
        Category::SmallStraight => {
            let ok = (1..=3).any(|start| (start..start + 4).all(|n| has(n)));
            if joker || ok { 30 } else { 0 }
        }
        Category::LargeStraight => {
            let ok = (1..=2).any(|start| (start..start + 5).all(|n| has(n)));
            if joker || ok { 40 } else { 0 }
        }
        Category::Yahtzee => {
            if max_count == 5 {
                50
            } else {
                0
            }
        }
        Category::Chance => sum,
    }
}

// ───────────────────────────── Players ─────────────────────────────

#[derive(Clone)]
struct Player {
    name: &'static str,
    scores: [Option<u32>; 13],
    /// Total Yahtzee-bonus points (100 each).
    bonus: u32,
}

impl Player {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            scores: [None; 13],
            bonus: 0,
        }
    }

    fn upper_sum(&self) -> u32 {
        self.scores[..6].iter().flatten().sum()
    }

    fn upper_bonus(&self) -> u32 {
        if self.upper_sum() >= 63 { 35 } else { 0 }
    }

    fn total(&self) -> u32 {
        self.scores.iter().flatten().sum::<u32>() + self.upper_bonus() + self.bonus
    }

    fn open(&self) -> [bool; 13] {
        std::array::from_fn(|i| self.scores[i].is_none())
    }

    fn done(&self) -> bool {
        self.scores.iter().all(|s| s.is_some())
    }
}

// ───────────────────────────── AI ─────────────────────────────

/// Best open category for a finished set of dice (score relative to baseline).
fn best_choice(dice: &[u8; 5], open: &[bool; 13], yahtzee_filled: bool) -> (Category, f32) {
    let joker = yahtzee_filled && is_yahtzee(dice);
    let mut best = (Category::Chance, f32::MIN);
    for c in CATEGORIES {
        if !open[c.index()] {
            continue;
        }
        let v = points(c, dice, joker) as f32 - c.baseline();
        if v > best.1 {
            best = (c, v);
        }
    }
    best
}

/// Expected value of keeping the dice whose bit is set in `mask` and rerolling the rest once.
fn ev_for_mask(dice: &[u8; 5], mask: u8, open: &[bool; 13], yahtzee_filled: bool) -> f32 {
    let idx: Vec<usize> = (0..5).filter(|i| (mask >> i) & 1 == 0).collect();
    let total = 6usize.pow(idx.len() as u32);
    let mut d = *dice;
    let mut sum = 0.0;
    for n in 0..total {
        let mut x = n;
        for &i in &idx {
            d[i] = (x % 6) as u8 + 1;
            x /= 6;
        }
        sum += best_choice(&d, open, yahtzee_filled).1;
    }
    sum / total as f32
}

// ───────────────────────────── Game state ─────────────────────────────

struct Yahtzee {
    players: [Player; 2],
    /// 0 = human, 1 = AI
    current: usize,
    /// Top row: dice rolled this turn (sorted ascending).
    rolled: Vec<u8>,
    /// Bottom row: kept dice (sorted ascending).
    kept: Vec<u8>,
    rolls_left: u8,
    log: String,
    /// Dice shown (sorted) after the last scoring message.
    log_dice: Vec<u8>,
    log_bonus: bool,
    /// AI has moved its held dice and the next tick should roll.
    ai_hold_applied: bool,
}

impl Default for Yahtzee {
    fn default() -> Self {
        Self {
            players: [Player::new("You"), Player::new("AI")],
            current: 0,
            rolled: Vec::new(),
            kept: Vec::new(),
            rolls_left: 3,
            log: String::from("Click Roll to start your turn."),
            log_dice: Vec::new(),
            log_bonus: false,
            ai_hold_applied: false,
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    Roll,
    ToggleRolled(usize),
    ToggleKept(usize),
    Score(Category),
    AutoScore,
    AutoNumber,
    NewGame,
    AiTick,
}

impl Yahtzee {
    fn game_over(&self) -> bool {
        self.players.iter().all(|p| p.done())
    }

    fn human_turn(&self) -> bool {
        self.current == 0 && !self.game_over()
    }

    fn has_rolled(&self) -> bool {
        self.rolls_left < 3 && self.rolled.len() + self.kept.len() == 5
    }

    fn all_dice(&self) -> [u8; 5] {
        let mut d = [0u8; 5];
        for (i, &v) in self
            .rolled
            .iter()
            .chain(self.kept.iter())
            .enumerate()
            .take(5)
        {
            d[i] = v;
        }
        d.sort();
        d
    }

    fn can_roll(&self) -> bool {
        self.rolls_left > 0 && self.kept.len() < 5
    }

    fn roll(&mut self) {
        if !self.can_roll() {
            return;
        }
        let n = 5 - self.kept.len();
        let mut rng = rand::rng();
        self.rolled = (0..n).map(|_| rng.random_range(1..=6)).collect();
        self.rolled.sort();
        self.rolls_left -= 1;
        self.ai_hold_applied = false;
    }

    fn score(&mut self, cat: Category) {
        if !self.has_rolled() || self.players[self.current].scores[cat.index()].is_some() {
            return;
        }
        let dice = self.all_dice();
        let yz = is_yahtzee(&dice);
        let p = &self.players[self.current];
        let joker = yz && p.scores[YAHTZEE_IDX].is_some();
        let bonus = yz && p.scores[YAHTZEE_IDX] == Some(50);
        let pts = points(cat, &dice, joker);

        let p = &mut self.players[self.current];
        p.scores[cat.index()] = Some(pts);
        if bonus {
            p.bonus += 100;
        }

        self.log = format!("{} scored {} in {}", p.name, pts, cat.name());
        self.log_dice = dice.to_vec();
        self.log_bonus = bonus;

        self.rolled.clear();
        self.kept.clear();
        self.rolls_left = 3;
        self.ai_hold_applied = false;
        self.current = 1 - self.current;
    }

    fn ai_tick(&mut self) {
        if self.rolls_left == 3 || self.ai_hold_applied {
            self.roll();
            return;
        }

        let dice = self.all_dice();
        let p = &self.players[1];
        let open = p.open();
        let yfilled = p.scores[YAHTZEE_IDX].is_some();

        if self.rolls_left == 0 {
            let (cat, _) = best_choice(&dice, &open, yfilled);
            self.score(cat);
            return;
        }

        // Try every hold pattern; mask 31 = keep everything (i.e. stop and score).
        let mut best_mask = 31u8;
        let mut best_ev = f32::MIN;
        for mask in (0..32u8).rev() {
            let ev = ev_for_mask(&dice, mask, &open, yfilled);
            if ev > best_ev + 1e-4 {
                best_ev = ev;
                best_mask = mask;
            }
        }

        if best_mask == 31 {
            let (cat, _) = best_choice(&dice, &open, yfilled);
            self.score(cat);
        } else {
            let mut kept = Vec::new();
            let mut rolled = Vec::new();
            for i in 0..5 {
                if (best_mask >> i) & 1 == 1 {
                    kept.push(dice[i]);
                } else {
                    rolled.push(dice[i]);
                }
            }
            kept.sort();
            rolled.sort();
            self.kept = kept;
            self.rolled = rolled;
            self.ai_hold_applied = true;
        }
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Roll => {
                if self.human_turn() {
                    self.roll();
                }
            }
            Message::ToggleRolled(i) => {
                if self.human_turn() && i < self.rolled.len() {
                    let d = self.rolled.remove(i);
                    self.kept.push(d);
                    self.kept.sort();
                }
            }
            Message::ToggleKept(i) => {
                if self.human_turn() && i < self.kept.len() {
                    let d = self.kept.remove(i);
                    self.rolled.push(d);
                    self.rolled.sort();
                }
            }
            Message::Score(cat) => {
                if self.human_turn() {
                    self.score(cat);
                }
            }
            Message::AutoScore => {
                if self.human_turn()
                    && self.has_rolled()
                    && let Some(cat) = self.best_scoring_category()
                {
                    self.score(cat);
                }
            }
            Message::AutoNumber => {
                if self.human_turn()
                    && self.has_rolled()
                    && let Some(cat) = self.best_number_category()
                {
                    self.score(cat);
                }
            }
            Message::NewGame => *self = Self::default(),
            Message::AiTick => {
                if self.current == 1 && !self.game_over() {
                    self.ai_tick();
                }
            }
        }
    }

    /// Open category with the most points for the current dice
    /// (ties go to the category that's cheapest to give up).
    fn best_scoring_category(&self) -> Option<Category> {
        let dice = self.all_dice();
        let p = &self.players[self.current];
        let joker = is_yahtzee(&dice) && p.scores[YAHTZEE_IDX].is_some();
        CATEGORIES
            .into_iter()
            .filter(|c| p.scores[c.index()].is_none())
            .max_by(|a, b| {
                let key = |c: &Category| (points(*c, &dice, joker) as f32, -c.baseline());
                let (ka, kb) = (key(a), key(b));
                ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// Open upper-section box (Ones–Sixes) whose number appears most often in the dice
    /// (ties go to the higher number, since it's worth more).
    fn best_number_category(&self) -> Option<Category> {
        let dice = self.all_dice();
        let p = &self.players[self.current];
        CATEGORIES[..6]
            .iter()
            .copied()
            .filter(|c| p.scores[c.index()].is_none())
            .max_by_key(|c| {
                let face = c.index() + 1;
                (dice.iter().filter(|&&d| d as usize == face).count(), face)
            })
    }

    fn theme(&self) -> Theme {
        Theme::custom(
            "Yahtzee".to_string(),
            Palette {
                background: Color::BLACK,
                text: Color::WHITE,
                primary: ACCENT,
                success: ACCENT,
                warning: Color::from_rgb(1.0, 0.8, 0.0),
                danger: Color::from_rgb(1.0, 0.3, 0.3),
            },
        )
    }

    fn subscription(&self) -> Subscription<Message> {
        if self.current == 1 && !self.game_over() {
            time::every(Duration::from_millis(900)).map(|_| Message::AiTick)
        } else {
            Subscription::none()
        }
    }

    // ───────────────────────────── View ─────────────────────────────

    fn view(&self) -> Element<'_, Message> {
        let human = self.human_turn();

        let status = if self.game_over() {
            let (a, b) = (self.players[0].total(), self.players[1].total());
            if a > b {
                format!("Game over — you win {a} to {b}!")
            } else if b > a {
                format!("Game over — AI wins {b} to {a}.")
            } else {
                format!("Game over — tie at {a}!")
            }
        } else if human {
            if self.rolls_left == 3 {
                "Your turn — roll the dice".to_string()
            } else {
                format!(
                    "Your turn — {} {} left",
                    self.rolls_left,
                    if self.rolls_left == 1 {
                        "roll"
                    } else {
                        "rolls"
                    }
                )
            }
        } else {
            "AI is playing…".to_string()
        };

        let mut status_text = text(status).size(22);
        if human {
            status_text = status_text.color(ACCENT);
        }

        // Dice rows: always 5 slots, filled left-to-right from sorted vectors.
        let rolled_row = Row::with_children((0..5).map(|i| match self.rolled.get(i) {
            Some(&v) => die_button(v, false, (human).then_some(Message::ToggleRolled(i))),
            None => empty_slot(),
        }))
        .spacing(12);

        let kept_row = Row::with_children((0..5).map(|i| match self.kept.get(i) {
            Some(&v) => die_button(v, true, (human).then_some(Message::ToggleKept(i))),
            None => empty_slot(),
        }))
        .spacing(12);

        let roll_label = format!("Roll ({} left)", self.rolls_left);
        let roll_btn = button(
            text(roll_label)
                .size(22)
                .width(Length::Fill)
                .align_x(alignment::Horizontal::Center),
        )
        .width(Length::Fill)
        .padding([14, 24])
        .style(accent_button)
        .on_press_maybe((human && self.can_roll()).then_some(Message::Roll));

        let mut log_row = Row::new()
            .spacing(8)
            .height(30)
            .align_y(alignment::Vertical::Center)
            .push(text(&self.log).size(16));
        for &d in &self.log_dice {
            log_row = log_row.push(mini_die(d));
        }
        if self.log_bonus {
            log_row = log_row.push(text("+100 Yahtzee bonus!").size(16).color(ACCENT));
        }

        let can_auto = human && self.has_rolled();

        let auto_number_btn = button(
            text("Most dice")
                .size(22)
                .width(Length::Fill)
                .align_x(alignment::Horizontal::Center),
        )
        .width(Length::Fill)
        .padding([14, 24])
        .style(potential_style)
        .on_press_maybe(
            (can_auto && self.best_number_category().is_some()).then_some(Message::AutoNumber),
        );

        let auto_best_btn = button(
            text("Highest score")
                .size(22)
                .width(Length::Fill)
                .align_x(alignment::Horizontal::Center),
        )
        .width(Length::Fill)
        .padding([14, 24])
        .style(potential_style)
        .on_press_maybe(can_auto.then_some(Message::AutoScore));

        let auto_row = row![auto_number_btn, auto_best_btn].spacing(12);

        let new_game_btn = button(
            text("New game")
                .size(22)
                .width(Length::Fill)
                .align_x(alignment::Horizontal::Center),
        )
        .width(Length::Fill)
        .padding([14, 24])
        .style(new_game_style)
        .on_press(Message::NewGame);

        let left = column![
            status_text,
            log_row,
            rolled_row,
            kept_row,
            roll_btn,
            auto_row,
            new_game_btn,
        ]
        .spacing(18)
        .width(Length::Fill);

        let board = container(self.scoreboard())
            .width(350)
            .padding(12)
            .style(|_theme: &Theme| container::Style {
                border: Border {
                    color: Color::WHITE,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..container::Style::default()
            });

        row![left, board].spacing(24).padding(24).into()
    }

    fn scoreboard(&self) -> Element<'_, Message> {
        let mut col = Column::new().spacing(5);

        col = col.push(line(
            "",
            cell(text("You").size(17).into()),
            cell(text("AI").size(17).into()),
        ));
        col = col.push(divider());

        for cat in &CATEGORIES[..6] {
            col = col.push(self.category_line(*cat));
        }
        col = col.push(divider());
        col = col.push(line(
            "Upper subtotal (63+)",
            cell(
                text(self.players[0].upper_sum().to_string())
                    .size(15)
                    .into(),
            ),
            cell(
                text(self.players[1].upper_sum().to_string())
                    .size(15)
                    .into(),
            ),
        ));
        col = col.push(line(
            "Upper bonus (+35)",
            cell(
                text(self.players[0].upper_bonus().to_string())
                    .size(15)
                    .into(),
            ),
            cell(
                text(self.players[1].upper_bonus().to_string())
                    .size(15)
                    .into(),
            ),
        ));
        col = col.push(divider());

        for cat in &CATEGORIES[6..] {
            col = col.push(self.category_line(*cat));
        }
        col = col.push(divider());
        col = col.push(line(
            "Yahtzee bonus (+100)",
            cell(text(self.players[0].bonus.to_string()).size(15).into()),
            cell(text(self.players[1].bonus.to_string()).size(15).into()),
        ));
        col = col.push(divider());
        col = col.push(line(
            "TOTAL",
            cell(text(self.players[0].total().to_string()).size(20).into()),
            cell(text(self.players[1].total().to_string()).size(20).into()),
        ));

        col.into()
    }

    fn category_line(&self, cat: Category) -> Element<'_, Message> {
        line(cat.name(), self.score_cell(0, cat), self.score_cell(1, cat))
    }

    fn score_cell(&self, player: usize, cat: Category) -> Element<'_, Message> {
        let p = &self.players[player];
        if let Some(v) = p.scores[cat.index()] {
            return cell(text(v.to_string()).size(15).into());
        }

        // Human can click an open box to score the current dice there.
        if player == 0 && self.human_turn() && self.has_rolled() {
            let dice = self.all_dice();
            let joker = is_yahtzee(&dice) && p.scores[YAHTZEE_IDX].is_some();
            let pts = points(cat, &dice, joker);
            return cell(
                button(
                    text(pts.to_string())
                        .size(14)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .align_x(alignment::Horizontal::Center)
                        .align_y(alignment::Vertical::Center),
                )
                .width(56)
                .height(24)
                .padding(0)
                .style(potential_style)
                .on_press(Message::Score(cat))
                .into(),
            );
        }

        cell(text("").size(15).into())
    }
}

// ───────────────────────────── View helpers ─────────────────────────────

fn line<'a>(
    label: &str,
    you: Element<'a, Message>,
    ai: Element<'a, Message>,
) -> Element<'a, Message> {
    row![
        text(label.to_string()).size(15).width(Length::Fill),
        you,
        ai
    ]
    .align_y(alignment::Vertical::Center)
    .into()
}

fn divider<'a>() -> Element<'a, Message> {
    container(text(""))
        .width(Length::Fill)
        .height(1)
        .style(|_theme: &Theme| container::Style {
            background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.35))),
            ..container::Style::default()
        })
        .into()
}

fn cell<'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    container(content)
        .width(64)
        .height(28)
        .align_x(alignment::Horizontal::Center)
        .align_y(alignment::Vertical::Center)
        .into()
}

fn die_button<'a>(value: u8, kept: bool, on_press: Option<Message>) -> Element<'a, Message> {
    button(
        text(value.to_string())
            .size(36)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(alignment::Horizontal::Center)
            .align_y(alignment::Vertical::Center),
    )
    .width(68)
    .height(68)
    .on_press_maybe(on_press)
    .style(die_style(kept))
    .into()
}

fn darken(c: Color, by: f32) -> Color {
    Color::from_rgb(
        (c.r - by).max(0.0),
        (c.g - by).max(0.0),
        (c.b - by).max(0.0),
    )
}

fn die_style(kept: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let base = if kept { ACCENT } else { Color::WHITE };
        let bg = match status {
            button::Status::Hovered | button::Status::Pressed => darken(base, 0.15),
            _ => base,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: Color::BLACK,
            border: Border {
                color: bg,
                width: 2.0,
                radius: 12.0.into(),
            },
            ..button::Style::default()
        }
    }
}

/// Solid accent button (the Roll button).
fn accent_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (bg, fg) = match status {
        button::Status::Active => (ACCENT, Color::BLACK),
        button::Status::Hovered | button::Status::Pressed => (darken(ACCENT, 0.15), Color::BLACK),
        button::Status::Disabled => (
            Color::from_rgb(0.15, 0.15, 0.15),
            Color::from_rgb(0.5, 0.5, 0.5),
        ),
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: fg,
        border: Border {
            color: bg,
            width: 0.0,
            radius: 10.0.into(),
        },
        ..button::Style::default()
    }
}

/// Outlined white button (New game).
fn new_game_style(_theme: &Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: hovered.then_some(Background::Color(Color::WHITE)),
        text_color: if hovered { Color::BLACK } else { Color::WHITE },
        border: Border {
            color: Color::WHITE,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..button::Style::default()
    }
}

/// Outlined accent button (clickable potential scores on the scoreboard).
fn potential_style(_theme: &Theme, status: button::Status) -> button::Style {
    if matches!(status, button::Status::Disabled) {
        let dim = Color::from_rgb(0.4, 0.4, 0.4);
        return button::Style {
            background: None,
            text_color: dim,
            border: Border {
                color: dim,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..button::Style::default()
        };
    }
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: hovered.then_some(Background::Color(ACCENT)),
        text_color: if hovered { Color::BLACK } else { ACCENT },
        border: Border {
            color: ACCENT,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..button::Style::default()
    }
}

/// Small rounded-square die used in the scoring message.
fn mini_die<'a>(value: u8) -> Element<'a, Message> {
    container(
        text(value.to_string())
            .size(16)
            .color(Color::BLACK)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(alignment::Horizontal::Center)
            .align_y(alignment::Vertical::Center),
    )
    .width(28)
    .height(28)
    .style(|_theme: &Theme| container::Style {
        background: Some(Background::Color(Color::WHITE)),
        border: Border {
            color: Color::WHITE,
            width: 1.0,
            radius: 7.0.into(),
        },
        ..container::Style::default()
    })
    .into()
}

fn empty_slot<'a>() -> Element<'a, Message> {
    container(text(""))
        .width(68)
        .height(68)
        .style(|_theme: &Theme| container::Style {
            border: Border {
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.5),
                width: 2.0,
                radius: 12.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}
