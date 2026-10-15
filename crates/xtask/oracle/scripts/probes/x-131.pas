{ parity probe: x-131 }
unit P_x_131;

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
  AddMessage(IntToStr(Pos(5, '125')));
  AddMessage('after');
end;
end.
