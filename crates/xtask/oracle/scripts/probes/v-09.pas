{ parity probe: v-09 }
unit P_v_09;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := 'a' + 'b';
  Show('''a'' + ''b''', v);
end;
end.
