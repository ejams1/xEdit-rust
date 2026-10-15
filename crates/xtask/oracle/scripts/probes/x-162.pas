{ parity probe: x-162 }
unit P_x_162;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  v := 'abc'; v := v = Unassigned; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
