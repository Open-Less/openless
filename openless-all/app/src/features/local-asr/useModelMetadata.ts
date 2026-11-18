import { useEffect, useRef, useState, type MutableRefObject } from 'react';
import {
  fetchLocalAsrHfCard,
  fetchLocalAsrRemoteInfo,
  fetchSherpaOnnxAsrRemoteInfo,
  type LocalAsrRemoteInfo,
  type SherpaOnnxRemoteInfo,
  type HfModelCard,
} from '../../api/local-asr';
import { LocalModelMetadataCache } from '../../lib/localModelMetadataCache';
import type { RemoteSize } from './types';

type RefreshGuard = () => boolean;

const remoteInfoCache = new LocalModelMetadataCache<LocalAsrRemoteInfo>();
const sherpaInfoCache = new LocalModelMetadataCache<SherpaOnnxRemoteInfo>();
const modelCardCache = new LocalModelMetadataCache<HfModelCard>();

export function useModelMetadata(
  mirror: string | undefined,
  refreshGenerationRef: MutableRefObject<number>,
) {
  const [remoteSizes, setRemoteSizes] = useState<Record<string, RemoteSize>>({});
  // HF model cards (downloads/stars/description) shown on the right of the dialog; successful
  // results are cached, failures record { loading:false, error } to allow retry.
  const [hfCards, setHfCards] = useState<
    Record<string, HfModelCard | { loading: boolean; error: string | null }>
  >({});
  const [sherpaRemoteSizes, setSherpaRemoteSizes] = useState<Record<string, RemoteSize>>({});
  const metadataMirrorRef = useRef(mirror);
  metadataMirrorRef.current = mirror;
  const makeMetadataGuard = (): RefreshGuard => {
    const generation = refreshGenerationRef.current;
    const mirror = metadataMirrorRef.current;
    return () =>
      generation === refreshGenerationRef.current && mirror === metadataMirrorRef.current;
  };
  // Changing the source invalidates displayed metadata, not the local catalog.
  useEffect(() => {
    setRemoteSizes({});
    setSherpaRemoteSizes({});
    setHfCards({});
  }, [mirror]);

  const ensureRemoteSize = async (modelId: string, mirror: string) => {
    const isCurrent = makeMetadataGuard();
    if (!isCurrent()) return;
    setRemoteSizes((prev) => {
      if (prev[modelId] && !prev[modelId].error) return prev;
      return {
        ...prev,
        [modelId]: {
          totalBytes: 0,
          fileCount: 0,
          loading: true,
          error: null,
        },
      };
    });
    try {
      const info = await remoteInfoCache.load(JSON.stringify([modelId, mirror]), () =>
        fetchLocalAsrRemoteInfo(modelId, mirror),
      );
      if (!isCurrent()) return;
      setRemoteSizes((prev) => ({
        ...prev,
        [modelId]: {
          totalBytes: info.totalBytes,
          fileCount: info.files.length,
          loading: false,
          error: null,
        },
      }));
    } catch (e) {
      if (!isCurrent()) return;
      setRemoteSizes((prev) => ({
        ...prev,
        [modelId]: {
          totalBytes: 0,
          fileCount: 0,
          loading: false,
          error: e instanceof Error ? e.message : String(e),
        },
      }));
    }
  };

  // HF model cards are fetched on demand (when a model is selected in the dialog); successful results are cached.
  const ensureHfCard = async (modelId: string, mirror: string) => {
    const isCurrent = makeMetadataGuard();
    setHfCards((prev) => ({
      ...prev,
      [modelId]: { loading: true, error: null },
    }));
    try {
      const card = await modelCardCache.load(JSON.stringify([modelId, mirror]), () =>
        fetchLocalAsrHfCard(modelId, mirror),
      );
      if (!isCurrent()) return;
      setHfCards((prev) => ({ ...prev, [modelId]: card }));
    } catch (e) {
      if (!isCurrent()) return;
      setHfCards((prev) => ({
        ...prev,
        [modelId]: {
          loading: false,
          error: e instanceof Error ? e.message : String(e),
        },
      }));
    }
  };

  const ensureSherpaRemoteSize = async (modelAlias: string, mirror: string) => {
    const isCurrent = makeMetadataGuard();
    if (!isCurrent()) return;
    setSherpaRemoteSizes((prev) => {
      if (prev[modelAlias] && !prev[modelAlias].error) return prev;
      return {
        ...prev,
        [modelAlias]: {
          totalBytes: 0,
          fileCount: 0,
          loading: true,
          error: null,
        },
      };
    });
    try {
      const info = await sherpaInfoCache.load(JSON.stringify([modelAlias, mirror]), () =>
        fetchSherpaOnnxAsrRemoteInfo(modelAlias, mirror),
      );
      if (!isCurrent()) return;
      setSherpaRemoteSizes((prev) => ({
        ...prev,
        [modelAlias]: {
          totalBytes: info.totalBytes,
          fileCount: info.files.length,
          loading: false,
          error: null,
        },
      }));
    } catch (e) {
      if (!isCurrent()) return;
      setSherpaRemoteSizes((prev) => ({
        ...prev,
        [modelAlias]: {
          totalBytes: 0,
          fileCount: 0,
          loading: false,
          error: e instanceof Error ? e.message : String(e),
        },
      }));
    }
  };

  const clearModelMetadata = () => {
    remoteInfoCache.clear();
    sherpaInfoCache.clear();
    modelCardCache.clear();
  };

  return {
    remoteSizes,
    sherpaRemoteSizes,
    hfCards,
    ensureRemoteSize,
    ensureHfCard,
    ensureSherpaRemoteSize,
    clearModelMetadata,
  };
}
