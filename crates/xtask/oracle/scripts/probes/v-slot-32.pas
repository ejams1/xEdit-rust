{ parity probe: v-slot-32 }
unit P_v_slot_32;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := VarType(Pos('b', 'ab'));
  Show('VarType(Pos(''b'', ''ab''))', v);
end;
end.
