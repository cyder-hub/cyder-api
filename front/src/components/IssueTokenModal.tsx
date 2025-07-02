import { createSignal, Show, Component } from 'solid-js';
import {
    DialogRoot,
    DialogContent,
    DialogHeader,
    DialogFooter,
    DialogTitle,
    DialogDescription,
} from './ui/Dialog';
import { Button } from './ui/Button';
import { TextField } from './ui/Input';
import { Select } from './ui/Select';
import { useI18n } from '../i18n';
import { request } from '../services/api';
import type { ApiKeyItem } from '../store/types';

interface IssueTokenModalProps {
    isOpen: () => boolean;
    onClose: () => void;
    apiKey: ApiKeyItem | null;
}

const IssueTokenModal: Component<IssueTokenModalProps> = (props) => {
    const [t] = useI18n();
    const [uid, setUid] = createSignal('');
    const [channel, setChannel] = createSignal('');
    const [duration, setDuration] = createSignal('1y');
    const [generatedToken, setGeneratedToken] = createSignal<string | null>(null);
    const [error, setError] = createSignal<string | null>(null);

    const durationOptions = [
        { value: '1d', label: t('issueTokenModal.durations.1d') },
        { value: '7d', label: t('issueTokenModal.durations.7d') },
        { value: '30d', label: t('issueTokenModal.durations.30d') },
        { value: '1y', label: t('issueTokenModal.durations.1y') },
        { value: '3y', label: t('issueTokenModal.durations.3y') },
        { value: 'forever', label: t('issueTokenModal.durations.forever') },
    ];

    const resetState = () => {
        setUid('');
        setChannel('');
        setDuration('1y');
        setGeneratedToken(null);
        setError(null);
    };

    const handleClose = () => {
        resetState();
        props.onClose();
    };

    const handleSubmit = async () => {
        if (!props.apiKey) return;
        if (!uid()) {
            setError(t('issueTokenModal.uidRequired'));
            return;
        }
        setError(null);

        let payload: { uid: string; channel?: string; duration?: number; end_at?: number } = {
            uid: uid(),
        };

        if (channel()) {
            payload.channel = channel();
        }

        const d = duration();
        if (d === 'forever') {
            payload.end_at = 253402297199000; // Year 9999
        } else {
            const day_ms = 24 * 60 * 60 * 1000;
            const durationMap: { [key: string]: number } = {
                '1d': 1 * day_ms,
                '7d': 7 * day_ms,
                '30d': 30 * day_ms,
                '1y': 365 * day_ms,
                '3y': 3 * 365 * day_ms,
            };
            payload.duration = durationMap[d];
        }

        try {
            const response = await request<string>(`/ai/manager/api/system_api_key/${props.apiKey.id}/issue`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(payload),
            });
            setGeneratedToken(response);
        } catch (err) {
            setError((err as Error).message || t('unknownError'));
        }
    };
    

    return (
        <DialogRoot open={props.isOpen()} onOpenChange={(isOpen) => !isOpen && handleClose()}>
            <DialogContent>
                <DialogHeader>
                    <DialogTitle>{t('issueTokenModal.title')}</DialogTitle>
                    <DialogDescription>
                        {t('issueTokenModal.description', { name: props.apiKey?.name || '' })}
                    </DialogDescription>
                </DialogHeader>

                <Show when={!generatedToken()}>
                    <div class="space-y-4 py-4">
                        <TextField
                            label={t('issueTokenModal.uidLabel')}
                            value={uid()}
                            onChange={setUid}
                            placeholder={t('issueTokenModal.uidPlaceholder')}
                            required
                        />
                        <TextField
                            label={t('issueTokenModal.channelLabel')}
                            value={channel()}
                            onChange={setChannel}
                            placeholder={t('issueTokenModal.channelPlaceholder')}
                        />
                        <Select
                            value={durationOptions.find(item => item.value === duration())}
                            onChange={(v) => setDuration(v.value)}
                            options={durationOptions}
                            placeholder={t('issueTokenModal.durationPlaceholder')}
                            optionValue="value"
                            optionTextValue="label"
                            label={t('issueTokenModal.durationLabel')}
                        />
                        <Show when={error()}>
                            <p class="text-sm text-red-600">{error()}</p>
                        </Show>
                    </div>
                </Show>

                <Show when={generatedToken()}>
                    <div class="space-y-4 py-4">
                        <p>{t('issueTokenModal.tokenGenerated')}</p>
                        <TextField
                            textarea
                            rows={8}
                            value={generatedToken()!}
                            readOnly
                        />
                    </div>
                </Show>

                <DialogFooter>
                    <Button variant="secondary" onClick={handleClose}>{t('common.cancel')}</Button>
                    <Show when={!generatedToken()}>
                        <Button onClick={handleSubmit}>{t('issueTokenModal.issueButton')}</Button>
                    </Show>
                </DialogFooter>
            </DialogContent>
        </DialogRoot>
    );
};

export default IssueTokenModal;
