{ parity probe: v-slot-25 }
unit P_v_slot_25;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  w: Word;
begin
  w := 70000;
  v := w;
  Show('w', v);
end;
end.
