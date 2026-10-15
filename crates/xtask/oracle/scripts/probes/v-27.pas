{ parity probe: v-27 }
unit P_v_27;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := not 'x';
  Show('not ''x''', v);
end;
end.
