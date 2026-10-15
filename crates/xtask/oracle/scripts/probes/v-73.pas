{ parity probe: v-73 }
unit P_v_73;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := 1.5 = '1.5';
  Show('1.5 = ''1.5''', v);
end;
end.
