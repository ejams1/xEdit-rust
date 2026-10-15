{ parity probe: v-25 }
unit P_v_25;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := - '5';
  Show('- ''5''', v);
end;
end.
