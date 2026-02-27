use crate::session::SessionState;
use chrono::{DateTime, Utc};
use regex::Regex;

#[derive(Debug, Clone)]
pub struct StateDetector {
    idle_threshold_secs: i64,
    waiting_threshold_secs: i64,
    prompt_patterns: Vec<Regex>,
    ansi_re: Regex,
}

#[derive(Debug, Clone)]
pub struct StateSignal {
    pub process_alive: bool,
    pub exit_code: Option<i32>,
    pub last_output_at: DateTime<Utc>,
    pub recent_output: String,
    pub waiting_hint: bool,
}

impl StateDetector {
    pub fn new(
        idle_threshold_secs: i64,
        waiting_threshold_secs: i64,
        prompt_patterns: Vec<Regex>,
    ) -> Self {
        Self {
            idle_threshold_secs,
            waiting_threshold_secs,
            prompt_patterns,
            ansi_re: Regex::new(r"\x1b\[[0-9;]*[A-Za-z]").unwrap(),
        }
    }

    pub fn detect(&self, now: DateTime<Utc>, signal: &StateSignal) -> SessionState {
        if !signal.process_alive {
            return match signal.exit_code {
                Some(0) => SessionState::Terminated,
                Some(_) => SessionState::Errored,
                None => SessionState::Errored,
            };
        }

        if signal.waiting_hint {
            return SessionState::WaitingInput;
        }

        let idle_secs = (now - signal.last_output_at).num_seconds();
        let cleaned_output = self.ansi_re.replace_all(&signal.recent_output, "");
        let waiting_prompt = self
            .prompt_patterns
            .iter()
            .any(|pat| pat.is_match(&cleaned_output));

        if waiting_prompt && idle_secs >= self.waiting_threshold_secs {
            return SessionState::WaitingInput;
        }

        if idle_secs >= self.idle_threshold_secs {
            return SessionState::Idle;
        }

        SessionState::Running
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_detector_marks_waiting_input() {
        let detector = StateDetector::new(60, 10, vec![Regex::new(r"(?m)^>\s*$").unwrap()]);
        let signal = StateSignal {
            process_alive: true,
            exit_code: None,
            last_output_at: Utc::now() - chrono::Duration::seconds(15),
            recent_output: "\u{1b}[32m>\u{1b}[0m".to_string(),
            waiting_hint: false,
        };
        let state = detector.detect(Utc::now(), &signal);
        assert_eq!(state, SessionState::WaitingInput);
    }

    #[test]
    fn state_detector_marks_idle() {
        let detector = StateDetector::new(30, 10, vec![]);
        let signal = StateSignal {
            process_alive: true,
            exit_code: None,
            last_output_at: Utc::now() - chrono::Duration::seconds(45),
            recent_output: "".to_string(),
            waiting_hint: false,
        };
        let state = detector.detect(Utc::now(), &signal);
        assert_eq!(state, SessionState::Idle);
    }

    #[test]
    fn state_detector_honors_waiting_hint() {
        let detector = StateDetector::new(30, 10, vec![]);
        let signal = StateSignal {
            process_alive: true,
            exit_code: None,
            last_output_at: Utc::now() - chrono::Duration::seconds(1),
            recent_output: "".to_string(),
            waiting_hint: true,
        };
        let state = detector.detect(Utc::now(), &signal);
        assert_eq!(state, SessionState::WaitingInput);
    }

    #[test]
    fn state_detector_marks_running() {
        let detector = StateDetector::new(30, 10, vec![]);
        let signal = StateSignal {
            process_alive: true,
            exit_code: None,
            last_output_at: Utc::now() - chrono::Duration::seconds(5),
            recent_output: "".to_string(),
            waiting_hint: false,
        };
        let state = detector.detect(Utc::now(), &signal);
        assert_eq!(state, SessionState::Running);
    }

    #[test]
    fn state_detector_marks_errored() {
        let detector = StateDetector::new(30, 10, vec![]);
        let signal = StateSignal {
            process_alive: false,
            exit_code: Some(1),
            last_output_at: Utc::now(),
            recent_output: "".to_string(),
            waiting_hint: false,
        };
        let state = detector.detect(Utc::now(), &signal);
        assert_eq!(state, SessionState::Errored);
    }
}
