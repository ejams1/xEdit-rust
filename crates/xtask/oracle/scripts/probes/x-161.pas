{ parity probe: x-161 }
unit P_x_161;

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
  v := 'abc'; v := v = Null; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
