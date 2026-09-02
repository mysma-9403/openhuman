import { useT } from '../../../lib/i18n/I18nContext';
import { BILLING_DASHBOARD_URL } from '../../../utils/links';
import { openUrl } from '../../../utils/openUrl';
import Button from '../../ui/Button';
import { useSettingsNavigation } from '../hooks/useSettingsNavigation';
import SettingsPanel from '../layout/SettingsPanel';

const BillingPanel = () => {
  const { t } = useT();
  const { navigateBack } = useSettingsNavigation();

  return (
    // The description rides the scaffold's own slot: SettingsPanel already
    // renders the page h1 (from the settings route registry), so a second
    // `text-2xl` heading here stacked two page titles on one page.
    <SettingsPanel description={t('settings.billing.movedToWebDesc')}>
      <p className="text-xs font-semibold uppercase tracking-wide text-content-muted">
        {t('settings.billing.movedToWeb')}
      </p>

      <div className="flex flex-wrap gap-3">
        <Button
          type="button"
          variant="primary"
          size="md"
          onClick={() => {
            void openUrl(BILLING_DASHBOARD_URL);
          }}>
          {t('settings.billing.openDashboard')}
        </Button>
        <Button type="button" variant="secondary" size="md" onClick={navigateBack}>
          {t('settings.billing.backToSettings')}
        </Button>
      </div>
    </SettingsPanel>
  );
};

export default BillingPanel;
