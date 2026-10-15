{ parity probe: v-74 }
unit P_v_74;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := '1.5' < 2;
  Show('''1.5'' < 2', v);
end;
end.
