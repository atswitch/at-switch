import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import type { AppLanguage } from "./types";

const LANGUAGE_STORAGE_KEY = "at-switch-language";

type LanguageContextValue = {
  language: AppLanguage;
  setLanguage: (language: AppLanguage) => void;
  text: (chinese: string, english: string) => string;
};

const LanguageContext = createContext<LanguageContextValue>({
  language: "zh-CN",
  setLanguage: () => undefined,
  text: (chinese) => chinese,
});

function initialLanguage(): AppLanguage {
  const saved = window.localStorage.getItem(LANGUAGE_STORAGE_KEY);
  if (saved === "zh-CN" || saved === "en") return saved;
  return "zh-CN";
}

export function LanguageProvider({ children }: { children: React.ReactNode }) {
  const [language, setLanguageState] = useState<AppLanguage>(initialLanguage);

  const setLanguage = useCallback((nextLanguage: AppLanguage) => {
    setLanguageState(nextLanguage);
    window.localStorage.setItem(LANGUAGE_STORAGE_KEY, nextLanguage);
  }, []);

  useEffect(() => {
    document.documentElement.lang = language;
  }, [language]);

  const value = useMemo<LanguageContextValue>(
    () => ({
      language,
      setLanguage,
      text: (chinese, english) => (language === "zh-CN" ? chinese : english),
    }),
    [language, setLanguage],
  );

  return (
    <LanguageContext.Provider value={value}>
      {children}
    </LanguageContext.Provider>
  );
}

export function useLanguage(): LanguageContextValue {
  return useContext(LanguageContext);
}
