import { useNavigate } from '@solidjs/router';
import { onMount } from 'solid-js';

export default function RedirectToDashboard() {
    const navigate = useNavigate();
    onMount(() => {
        navigate('/dashboard', { replace: true });
    });
    return null;
}
