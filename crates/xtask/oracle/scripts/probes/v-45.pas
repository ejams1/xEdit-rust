{ parity probe: v-45 }
unit P_v_45;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := True / 2;
  Show('True / 2', v);
end;
end.
