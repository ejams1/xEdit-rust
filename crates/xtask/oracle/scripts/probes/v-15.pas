{ parity probe: v-15 }
unit P_v_15;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := '7' div 2;
  Show('''7'' div 2', v);
end;
end.
