{ parity probe: v-16 }
unit P_v_16;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := '7' mod 2;
  Show('''7'' mod 2', v);
end;
end.
