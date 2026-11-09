import React, { createContext, useContext, useState, useEffect, useCallback, useRef } from 'react';
import type {
  ViewTab,
  Session,
  PluginWithState,
  ProviderDef,
  ProviderModel,
  CustomProvider,
  AppSettings,
} from '../types';
import * as tauriService from '../services/tauri';

interface Toast {
  id: string;
  type: 'info' | 'success' | 'warning' | 'error';
  message: string;
}

interface AppContextType {
  activeTab: ViewTab;
  setActiveTab: (tab: ViewTab) => void;
  sessions: Session[];
  activeSessionId: string | null;
  setActiveSessionId: (id: string | null) => void;
  settings: AppSettings;
  updateSetting: (key: string, value: string) => Promise<void>;
  providers: ProviderDef[];
  updateProviderModels: (providerId: string, models: ProviderModel[]) => Promise<void>;
  customProviders: CustomProvider[];
  addCustomProvider: (provider: CustomProvider) => Promise<void>;
  updateCustomProvider: (provider: CustomProvider) => Promise<void>;
  deleteCustomProvider: (id: string) => Promise<void>;
  plugins: PluginWithState[];
  togglePlugin: (id: string) => Promise<void>;
  refreshPlugins: () => Promise<void>;
  refreshSessions: () => Promise<void>;
  createSession: (title?: string) => Promise<string>;
  deleteSession: (id: string) => Promise<void>;
  renameSession: (id: string, newTitle: string) => Promise<void>;
  toasts: Toast[];
  showToast: (message: string, type?: Toast['type']) => void;
  removeToast: (id: string) => void;
  isSpeaking: boolean;
  speakText: (text: string) => Promise<void>;
  stopSpeaking: () => Promise<void>;
  isRecording: boolean;
  toggleRecording: (onTranscript: (text: string) => void) => void;
  isTelemetryOpen: boolean;
  toggleTelemetry: () => void;
  isStandbyMode: boolean;
  setIsStandbyMode: (val: boolean) => void;
}

const defaultSettings: AppSettings = {
  aiMode: 'api',
  selectedProvider: 'groq',
  selectedModel: 'llama-3.3-70b-versatile',
  temperature: '0.7',
  customInstructions: "You are Joren, an advanced AI PC assistant. Keep responses clear, helpful, intelligent, and friendly. Wrap all code in Markdown triple backticks.",
  nickname: '',
  occupation: '',
  moreAboutYou: '',
  tavilyApiKey: '',
  huggingfaceApiKey: '',
  customProviders: '[]',
  ttsVoice: 'hi-IN-SwaraNeural',
  ttsEngine: 'cloud',
  kokoroModel: 'kokoro-v1.0.int8.onnx',
  autoSpeak: 'false',
};

const AppContext = createContext<AppContextType | null>(null);

export const AppProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const [activeTab, setActiveTab] = useState<ViewTab>('chat');
  const [sessions, setSessions] = useState<Session[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [settings, setSettings] = useState<AppSettings>(defaultSettings);
  const [providers, setProviders] = useState<ProviderDef[]>(tauriService.DEFAULT_PROVIDERS);
  const [customProviders, setCustomProviders] = useState<CustomProvider[]>([]);
  const [plugins, setPlugins] = useState<PluginWithState[]>([]);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [isSpeaking, setIsSpeaking] = useState(false);
  const [isRecording, setIsRecording] = useState(false);
  const [isTelemetryOpen, setIsTelemetryOpen] = useState(true);
  const [isStandbyMode, setIsStandbyMode] = useState(false);
  const mediaRecorderRef = useRef<MediaRecorder | null>(null);
  const mediaStreamRef = useRef<MediaStream | null>(null);
  const ttsAbortControllerRef = useRef<AbortController | null>(null);

  const toggleTelemetry = useCallback(() => {
    setIsTelemetryOpen((prev) => !prev);
  }, []);

  const showToast = useCallback((message: string, type: Toast['type'] = 'info') => {
    const id = Math.random().toString(36).substring(2, 9);
    setToasts((prev) => [...prev, { id, type, message }]);
    setTimeout(() => {
      setToasts((prev) => prev.filter((t) => t.id !== id));
    }, 4000);
  }, []);

  const removeToast = useCallback((id: string) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  const refreshSessions = useCallback(async () => {
    try {
      const sess = await tauriService.getAllSessions();
      setSessions(sess);
      if (sess.length > 0 && !activeSessionId) {
        setActiveSessionId(sess[0].id);
      }
    } catch (e) {
      console.error('Failed to load sessions:', e);
    }
  }, [activeSessionId]);

  const refreshPlugins = useCallback(async () => {
    try {
      const p = await tauriService.getPlugins();
      setPlugins(p);
    } catch (e) {
      console.error('Failed to load plugins:', e);
    }
  }, []);

  const loadInitialData = useCallback(async () => {
    try {
      const savedSettings = await tauriService.getAllSettings();
      setSettings((prev) => ({
        ...prev,
        ...savedSettings,
      }));

      // Parse custom providers
      let customList: CustomProvider[] = [];
      try {
        if (savedSettings.customProviders) {
          customList = JSON.parse(savedSettings.customProviders);
        }
      } catch (err) {
        console.error('Failed to parse custom providers JSON:', err);
      }
      setCustomProviders(customList);

      // Load hardcoded providers from rust
      let baseProviders = tauriService.DEFAULT_PROVIDERS;
      try {
        const provRes = await tauriService.getProviders();
        if (provRes && provRes.providers && provRes.providers.length > 0) {
          baseProviders = provRes.providers;
        }
      } catch (e) {
        console.error('Failed to get providers from backend:', e);
      }

      // Check for user customized models on built-in providers
      baseProviders = baseProviders.map((bp) => {
        const customKey = `providerModels_${bp.id}`;
        if (savedSettings[customKey]) {
          try {
            const parsed = JSON.parse(savedSettings[customKey]);
            if (Array.isArray(parsed) && parsed.length > 0) {
              return { ...bp, models: parsed };
            }
          } catch {}
        }
        return bp;
      });

      // Convert custom providers into ProviderDef
      const customDefs: ProviderDef[] = customList.map((cp) => ({
        id: cp.id,
        name: cp.name,
        models: cp.models || [],
        isCustom: true,
        baseUrl: cp.baseUrl,
        apiKey: cp.apiKey,
      }));

      setProviders([...baseProviders, ...customDefs]);

      await refreshPlugins();

      const sess = await tauriService.getAllSessions();
      setSessions(sess);
      if (sess.length > 0) {
        setActiveSessionId(sess[0].id);
      } else {
        const newId = 'session_' + Date.now();
        await tauriService.createSession(newId, 'General Chat');
        setSessions([{ id: newId, title: 'General Chat' }]);
        setActiveSessionId(newId);
      }
    } catch (err) {
      console.error('Error loading initial data:', err);
    }
  }, [refreshPlugins]);

  useEffect(() => {
    loadInitialData();
  }, [loadInitialData]);

  const updateSetting = async (key: string, value: string) => {
    try {
      setSettings((prev) => ({ ...prev, [key]: value }));
      await tauriService.saveSetting(key, value);
    } catch (e: any) {
      showToast('Failed to save setting: ' + (e.message || e), 'error');
    }
  };

  const syncCustomProvidersToSettings = async (list: CustomProvider[]) => {
    setCustomProviders(list);
    const jsonStr = JSON.stringify(list);
    await updateSetting('customProviders', jsonStr);

    // Recompute providers list
    let baseProviders = tauriService.DEFAULT_PROVIDERS;
    try {
      const provRes = await tauriService.getProviders();
      if (provRes && provRes.providers && provRes.providers.length > 0) {
        baseProviders = provRes.providers;
      }
    } catch {}

    baseProviders = baseProviders.map((bp) => {
      const customKey = `providerModels_${bp.id}`;
      if (settings[customKey]) {
        try {
          const parsed = JSON.parse(settings[customKey]);
          if (Array.isArray(parsed) && parsed.length > 0) {
            return { ...bp, models: parsed };
          }
        } catch {}
      }
      return bp;
    });

    const customDefs: ProviderDef[] = list.map((cp) => ({
      id: cp.id,
      name: cp.name,
      models: cp.models || [],
      isCustom: true,
      baseUrl: cp.baseUrl,
      apiKey: cp.apiKey,
    }));

    setProviders([...baseProviders, ...customDefs]);
  };

  const updateProviderModels = async (providerId: string, models: ProviderModel[]) => {
    try {
      const isCustom = customProviders.some((cp) => cp.id === providerId);
      if (isCustom) {
        const updated = customProviders.map((cp) =>
          cp.id === providerId ? { ...cp, models } : cp
        );
        await syncCustomProvidersToSettings(updated);
      } else {
        await updateSetting(`providerModels_${providerId}`, JSON.stringify(models));
        setProviders((prev) =>
          prev.map((p) => (p.id === providerId ? { ...p, models } : p))
        );
      }
    } catch (err: any) {
      showToast('Failed to update models: ' + (err.message || err), 'error');
    }
  };

  const addCustomProvider = async (provider: CustomProvider) => {
    try {
      const updated = [...customProviders.filter((p) => p.id !== provider.id), provider];
      await syncCustomProvidersToSettings(updated);
      showToast(`Custom provider "${provider.name}" added!`, 'success');
    } catch (err: any) {
      showToast('Failed to add provider: ' + (err.message || err), 'error');
    }
  };

  const updateCustomProvider = async (provider: CustomProvider) => {
    try {
      const updated = customProviders.map((p) => (p.id === provider.id ? provider : p));
      await syncCustomProvidersToSettings(updated);
      showToast(`Custom provider "${provider.name}" updated!`, 'success');
    } catch (err: any) {
      showToast('Failed to update provider: ' + (err.message || err), 'error');
    }
  };

  const deleteCustomProvider = async (id: string) => {
    try {
      const updated = customProviders.filter((p) => p.id !== id);
      await syncCustomProvidersToSettings(updated);
      if (settings.selectedProvider === id) {
        await updateSetting('selectedProvider', 'groq');
        await updateSetting('selectedModel', 'llama-3.3-70b-versatile');
      }
      showToast('Custom provider removed', 'info');
    } catch (err: any) {
      showToast('Failed to delete provider: ' + (err.message || err), 'error');
    }
  };

  const stopSpeaking = useCallback(async () => {
    if (ttsAbortControllerRef.current) {
      ttsAbortControllerRef.current.abort();
      ttsAbortControllerRef.current = null;
    }
    try {
      await tauriService.voiceStopPlayback();
      await tauriService.ttsStop();
    } catch (e) {
      console.error('Stop TTS error:', e);
    } finally {
      setIsSpeaking(false);
    }
  }, []);

  const createSession = async (title?: string) => {
    await stopSpeaking();
    const newId = 'session_' + Date.now();
    const sessionTitle = title || ('Chat ' + (sessions.length + 1));
    try {
      await tauriService.createSession(newId, sessionTitle);
      setSessions((prev) => [{ id: newId, title: sessionTitle }, ...prev]);
      setActiveSessionId(newId);
      return newId;
    } catch (e: any) {
      showToast('Failed to create session: ' + (e.message || e), 'error');
      return newId;
    }
  };

  const deleteSession = async (id: string) => {
    await stopSpeaking();
    try {
      await tauriService.deleteSession(id);
      const remaining = sessions.filter((s) => s.id !== id);
      setSessions(remaining);
      if (activeSessionId === id) {
        setActiveSessionId(remaining.length > 0 ? remaining[0].id : null);
      }
      showToast('Session deleted', 'info');
    } catch (e: any) {
      showToast('Failed to delete session: ' + (e.message || e), 'error');
    }
  };

  const renameSession = async (id: string, newTitle: string) => {
    try {
      await tauriService.renameSession(id, newTitle);
      setSessions((prev) =>
        prev.map((s) => (s.id === id ? { ...s, title: newTitle } : s))
      );
      showToast('Session renamed', 'success');
    } catch (e: any) {
      showToast('Failed to rename session: ' + (e.message || e), 'error');
    }
  };

  const togglePlugin = async (id: string) => {
    try {
      const newState = await tauriService.togglePlugin(id);
      setPlugins((prev) =>
        prev.map((p) => (p.id === id ? { ...p, enabled: newState } : p))
      );
      showToast('Plugin ' + id + (newState ? ' enabled' : ' disabled'), 'info');
    } catch (e: any) {
      showToast('Failed to toggle plugin: ' + (e.message || e), 'error');
    }
  };

  const speakText = useCallback(async (text: string) => {
    const trimmed = text.trim();
    // BUG #6: Strict guard against error messages, JSON objects, and empty strings
    if (
      !trimmed ||
      trimmed.startsWith('LLM Error:') ||
      trimmed.startsWith('Error:') ||
      trimmed.startsWith('API Error:') ||
      trimmed.startsWith('{"error"') ||
      trimmed.startsWith('{\n  "error"')
    ) {
      setIsSpeaking(false);
      return;
    }

    if (ttsAbortControllerRef.current) {
      ttsAbortControllerRef.current.abort();
    }
    const currentController = new AbortController();
    ttsAbortControllerRef.current = currentController;

    try {
      setIsSpeaking(true);
      await tauriService.ttsSpeak(trimmed, settings.ttsVoice || 'en-US-ChristopherNeural');
    } catch (e: any) {
      if (ttsAbortControllerRef.current === currentController) {
        console.error('TTS error:', e);
        showToast('TTS error: ' + (e.message || e), 'error');
      }
    } finally {
      if (ttsAbortControllerRef.current === currentController) {
        ttsAbortControllerRef.current = null;
        setIsSpeaking(false);
      }
    }
  }, [settings.ttsEngine, settings.ttsVoice, settings.kokoroModel, showToast]);

  // Stop active TTS and Speech Recording when switching sessions or application views
  useEffect(() => {
    stopSpeaking();
    if (mediaRecorderRef.current && mediaRecorderRef.current.state !== 'inactive') {
      try {
        mediaRecorderRef.current.stop();
      } catch {}
      mediaRecorderRef.current = null;
    }
    if (mediaStreamRef.current) {
      try {
        mediaStreamRef.current.getTracks().forEach((t) => t.stop());
      } catch {}
      mediaStreamRef.current = null;
    }
    setIsRecording(false);
  }, [activeSessionId, activeTab, stopSpeaking]);

  const toggleRecording = useCallback((onTranscript: (text: string) => void) => {
    if (isRecording) {
      if (mediaRecorderRef.current && mediaRecorderRef.current.state !== 'inactive') {
        try {
          mediaRecorderRef.current.stop();
        } catch {}
      }
      if (mediaStreamRef.current) {
        try {
          mediaStreamRef.current.getTracks().forEach((t) => t.stop());
        } catch {}
        mediaStreamRef.current = null;
      }
      setIsRecording(false);
      return;
    }

    if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
      showToast('Microphone audio recording is not supported in this environment.', 'warning');
      return;
    }

    // Turn Interruption (Barge-in): Halt active speech output before opening microphone
    stopSpeaking();

    navigator.mediaDevices
      .getUserMedia({ audio: true })
      .then((stream) => {
        mediaStreamRef.current = stream;
        const mediaRecorder = new MediaRecorder(stream);
        const audioChunks: Blob[] = [];

        mediaRecorder.ondataavailable = (e) => {
          if (e.data && e.data.size > 0) {
            audioChunks.push(e.data);
          }
        };

        mediaRecorder.onstart = () => {
          setIsRecording(true);
          showToast('Listening... Speak now', 'info');
        };

        mediaRecorder.onstop = async () => {
          setIsRecording(false);
          mediaRecorderRef.current = null;
          if (mediaStreamRef.current) {
            mediaStreamRef.current.getTracks().forEach((t) => t.stop());
            mediaStreamRef.current = null;
          }

          if (audioChunks.length === 0) return;

          try {
            const audioBlob = new Blob(audioChunks, {
              type: mediaRecorder.mimeType || 'audio/webm',
            });
            const arrayBuffer = await audioBlob.arrayBuffer();
            const AudioContextClass =
              window.AudioContext || (window as any).webkitAudioContext;
            if (!AudioContextClass) {
              showToast('AudioContext not supported in this environment.', 'error');
              return;
            }
            const audioCtx = new AudioContextClass();
            const decoded = await audioCtx.decodeAudioData(arrayBuffer);
            const samples = Array.from(decoded.getChannelData(0));
            await audioCtx.close();

            showToast('Transcribing audio with online STT...', 'info');
            const transcript = await tauriService.transcribeAudio(
              samples,
              decoded.sampleRate,
              'en'
            );
            if (transcript && transcript.trim()) {
              onTranscript(transcript.trim());
              showToast('Speech recognized', 'info');
            } else {
              showToast('No speech detected in audio.', 'info');
            }
          } catch (err: any) {
            console.error('Audio transcription error:', err);
            const msg = err.message || String(err);
            if (msg.includes('API key') || msg.includes('not configured')) {
              showToast(
                'Please set your Groq or OpenAI API key in Settings -> Models & Providers for STT.',
                'warning'
              );
            } else {
              showToast('Voice transcription failed: ' + msg, 'error');
            }
          }
        };

        mediaRecorder.onerror = (e) => {
          console.error('MediaRecorder error:', e);
          setIsRecording(false);
          showToast('Microphone recording error', 'error');
        };

        mediaRecorderRef.current = mediaRecorder;
        mediaRecorder.start();
      })
      .catch((err: any) => {
        setIsRecording(false);
        console.error('Microphone access failed:', err);
        if (err.name === 'NotAllowedError' || err.name === 'PermissionDeniedError') {
          showToast('Microphone access denied. Please grant microphone permissions.', 'error');
        } else if (err.name === 'NotFoundError' || err.name === 'DevicesNotFoundError') {
          showToast('No microphone found. Please connect a microphone.', 'error');
        } else {
          showToast('Failed to start microphone: ' + (err.message || err), 'error');
        }
      });
  }, [isRecording, showToast, stopSpeaking]);

  return (
    <AppContext.Provider
      value={{
        activeTab,
        setActiveTab,
        sessions,
        activeSessionId,
        setActiveSessionId,
        settings,
        updateSetting,
        providers,
        updateProviderModels,
        customProviders,
        addCustomProvider,
        updateCustomProvider,
        deleteCustomProvider,
        plugins,
        togglePlugin,
        refreshPlugins,
        refreshSessions,
        createSession,
        deleteSession,
        renameSession,
        toasts,
        showToast,
        removeToast,
        isSpeaking,
        speakText,
        stopSpeaking,
        isRecording,
        toggleRecording,
        isTelemetryOpen,
        toggleTelemetry,
        isStandbyMode,
        setIsStandbyMode,
      }}
    >
      {children}
    </AppContext.Provider>
  );
};

export const useApp = () => {
  const context = useContext(AppContext);
  if (!context) {
    throw new Error('useApp must be used within an AppProvider');
  }
  return context;
};
